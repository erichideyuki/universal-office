//! Lexer + parser of the CANONICAL formula (`parse.ts`: English names, `,` between arguments, `.` decimal, `;`
//! between array-constant rows) in A1 or R1C1. The screen form is accepted too: `@x` (SINGLE), `A1#`
//! (ANCHORARRAY), `[@Col]`, names without `_xlfn.`.
//!
//! Excel precedence (strongest first): `:` → space (intersection) → `,` (union, only inside parentheses) →
//! negation → `%` → `^` → `* /` → `+ -` → `&` → comparisons, so `=-2^2` is 4 and `^` is left-associative.
//!
//! **No recursion.** The TS parser is recursive descent; this one runs the same grammar on an explicit stack of
//! continuations ([`Frame`]), so hostile nesting costs heap, not stack, and the tree depth limit
//! ([`MAX_AST_DEPTH`]) is checked as nodes are built (decision A2-2). Tokens, the frame stack and the argument
//! stack are reusable buffers ([`FormulaParser`]): parsing allocates nothing after warm-up except the arena
//! growth.
//!
//! The TS `RegExp`s are hand matchers over UTF-16 units with the same backtracking outcome (the `u` flag decodes
//! code points only for the `\p{L}`/`\p{N}` look-aheads; every other class is BMP).

use crate::a1::{ascii_ci_eq, code_point_at, col_from_letters, is_js_space};
use crate::ast::{Ast, BinOp, FnName, MAX_AST_DEPTH, Node, NodeId, Part, RefPart, SheetSpec, Span, StrIx, TableItem, UnaryOp, name_flags};
use crate::jstext::{digits_value, js_trim};
use crate::key::{MAX_COL, MAX_ROW};
use crate::strings::JsStr;
use crate::value::{ErrorCode, Scalar, StrRef};

/// Why a text is not a formula (`FormulaSyntaxError`; the TS messages are Portuguese and no consumer reads them:
/// only the class and the position are compared, decision A2-3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyntaxKind {
    /// `texto sem fim`
    UnterminatedText,
    /// `aba entre aspas sem fim`
    UnterminatedSheet,
    /// `aba entre aspas sem "!"`
    SheetWithoutBang,
    /// `referência inválida` (a `$` that starts no reference)
    InvalidRef,
    /// `número fora do intervalo do Excel`
    NumberOutOfRange,
    /// `caractere inesperado`
    UnexpectedChar,
    /// `referência estruturada sem "]"`
    UnclosedTable,
    /// `referência estruturada inválida`
    InvalidTable,
    /// `sobra na fórmula`
    Trailing,
    /// `referência inválida após a aba`
    InvalidAfterSheet,
    /// `falta ")"`
    MissingParen,
    /// `referência inválida após "!"`
    InvalidAfterBang,
    /// `operando esperado`
    OperandExpected,
    /// `esperado "," ou ")"`
    ExpectedSeparator,
    /// `constante de matriz inválida`
    InvalidArray,
    /// `linhas de tamanhos diferentes na constante de matriz`
    RaggedArray,
    /// The tree would be deeper than [`MAX_AST_DEPTH`] (not in the TS, decision A2-2).
    TooDeep,
    /// A `(` never closed at the end of the text: the TS reads past its token list and throws a `TypeError`
    /// (not a `FormulaSyntaxError`, so its engine and INDIRECT do not catch it; decision A2-4).
    UnclosedParenAtEnd,
}

/// `FormulaSyntaxError`: the kind and the position (UTF-16 index) the TS reports.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SyntaxError {
    /// What failed.
    pub kind: SyntaxKind,
    /// `pos`.
    pub pos: u32,
}

/// `ParseOptions`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ParseOptions<'a> {
    /// R1C1 references.
    pub r1c1: bool,
    /// R1C1 letters (`r1c1Letters`: row, column); `None` = R and C.
    pub letters: Option<(JsStr<'a>, JsStr<'a>)>,
    /// The formula's row (relative A1 references become offsets from it).
    pub row: u32,
    /// The formula's column.
    pub col: u32,
}

const ERROR_CODES: [(&str, ErrorCode); 16] = [
    ("#NULL!", ErrorCode::Null),
    ("#DIV/0!", ErrorCode::Div0),
    ("#VALUE!", ErrorCode::Value),
    ("#REF!", ErrorCode::Ref),
    ("#NAME?", ErrorCode::Name),
    ("#NUM!", ErrorCode::Num),
    ("#N/A", ErrorCode::NA),
    ("#GETTING_DATA", ErrorCode::GettingData),
    ("#SPILL!", ErrorCode::Spill),
    ("#CALC!", ErrorCode::Calc),
    ("#FIELD!", ErrorCode::Field),
    ("#BLOCKED!", ErrorCode::Blocked),
    ("#CONNECT!", ErrorCode::Connect),
    ("#UNKNOWN!", ErrorCode::Unknown),
    ("#BUSY!", ErrorCode::Busy),
    ("#PYTHON!", ErrorCode::Python),
];

/// `numberLiteralValue(raw)`: the value of a number literal as Excel reads it — TRUNCATED to 15 significant
/// digits; a subnormal or an overflow is refused (`None`). Returns `(v, truncated)`.
#[must_use]
pub fn number_literal_value(raw: JsStr<'_>) -> Option<(f64, bool)> {
    crate::jstext::with_u16(raw, number_literal_units)
}

fn number_literal_units(s: &[u16]) -> Option<(f64, bool)> {
    // /^(\d*)\.?(\d*)(?:[eE]([+-]?\d+))?$/
    let d1 = digit_len(s, 0);
    let mut i = d1;
    let mut d2s = i;
    if s.get(i) == Some(&u16::from(b'.')) {
        i += 1;
        d2s = i;
    }
    let d2 = digit_len(s, i);
    i += d2;
    let mut exp: Option<&[u16]> = None;
    if i < s.len() {
        if !matches!(s[i], 0x45 | 0x65) {
            return None;
        }
        let e0 = i + 1;
        let mut j = e0;
        if matches!(s.get(j), Some(&0x2b | &0x2d)) {
            j += 1;
        }
        let n = digit_len(s, j);
        if n == 0 || j + n != s.len() {
            return None;
        }
        exp = Some(&s[e0..j + n]);
    }
    let int = &s[..d1];
    let frac = &s[d2s..d2s + d2];
    let mut digits: Vec<u16> = Vec::with_capacity(d1 + d2);
    digits.extend_from_slice(int);
    digits.extend_from_slice(frac);
    // parseInt(m[3], 10): the sign and the digits (a huge exponent is a large double)
    let e = exp.map_or(0.0, |e| {
        let (neg, d) = match e[0] {
            0x2d => (true, &e[1..]),
            0x2b => (false, &e[1..]),
            _ => (false, e),
        };
        let v = digits_value(d);
        if neg { -v } else { v }
    });
    let mut point = d1 as f64 + e;
    let lead = digits.iter().take_while(|&&u| u == 0x30).count();
    digits.drain(..lead);
    point -= lead as f64;
    while digits.last() == Some(&0x30) {
        digits.pop();
    }
    let (v, truncated) = if digits.is_empty() {
        (0.0, false)
    } else if digits.len() > 15 {
        // Number(`0.${digits.slice(0, 15)}e${point}`)
        let mut t = String::from("0.");
        t.extend(digits[..15].iter().map(|&u| char::from(u as u8)));
        t.push('e');
        t.push_str(&uo_jsnum::format::to_js_string(point));
        (uo_jsnum::parse::to_number(&t), true)
    } else {
        let t: String = s.iter().map(|&u| char::from(u as u8)).collect();
        (uo_jsnum::parse::to_number(&t), false)
    };
    if !v.is_finite() || (v != 0.0 && v < 2.225_073_858_507_201_4e-308) {
        return None;
    }
    Some((v, truncated))
}

fn digit_len(s: &[u16], i: usize) -> usize {
    s.get(i..).map_or(0, |r| r.iter().take_while(|&&u| is_digit(u)).count())
}

const fn is_digit(u: u16) -> bool {
    matches!(u, 0x30..=0x39)
}

const fn is_alpha(u: u16) -> bool {
    matches!(u, 0x41..=0x5a | 0x61..=0x7a)
}

/// `IDENT_START` (one unit): `[\p{L}_\\]|[^\x00-\x7f\s 　]`.
const fn ident_start(u: u16) -> bool {
    is_alpha(u) || u == 0x5f || u == 0x5c || (u > 0x7f && !is_js_space(u))
}

/// `IDENT_RE`'s continuation: `[\p{L}\p{N}_.\\?]|[^\x00-\x7f\s 　]`.
const fn ident_char(u: u16) -> bool {
    is_alpha(u) || is_digit(u) || matches!(u, 0x5f | 0x2e | 0x5c | 0x3f) || (u > 0x7f && !is_js_space(u))
}

/// `SHEET_RE`'s name class `[^\s'"!:;,(){}[\]+\-*/^&=<>%@#]`.
const fn sheet_char(u: u16) -> bool {
    !(is_js_space(u)
        || matches!(
            u,
            0x27 | 0x22
                | 0x21
                | 0x3a
                | 0x3b
                | 0x2c
                | 0x28
                | 0x29
                | 0x7b
                | 0x7d
                | 0x5b
                | 0x5d
                | 0x2b
                | 0x2d
                | 0x2a
                | 0x2f
                | 0x5e
                | 0x26
                | 0x3d
                | 0x3c
                | 0x3e
                | 0x25
                | 0x40
                | 0x23
        ))
}

/// Ws set of the lexer: space, `\n`, `\r`, `\t`, NBSP.
const fn lex_space(u: u16) -> bool {
    matches!(u, 0x20 | 0x0a | 0x0d | 0x09 | 0xa0)
}

/// The negative look-ahead classes of the reference `RegExp`s at `e`: `\p{L}`, `\p{N}` and `extra` units, plus
/// (`dot_colon`) `\.(?!:)` or (`dot`) a plain `.`.
fn lookahead_ok(s: &[u16], e: usize, extra: &[u8], dot_colon: bool, dot: bool) -> bool {
    let Some((cp, _)) = code_point_at(s, e) else { return true };
    if uo_text::props::is_letter(cp) || uo_text::props::is_number(cp) || extra.iter().any(|&x| u32::from(x) == cp) {
        return false;
    }
    if cp == 0x2e {
        if dot {
            return false;
        }
        if dot_colon && s.get(e + 1) != Some(&0x3a) {
            return false;
        }
    }
    true
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Op {
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Amp,
    Eq,
    Lt,
    Gt,
    Ne,
    Le,
    Ge,
    Percent,
    Colon,
    Comma,
    Semi,
    LParen,
    RParen,
    LBrace,
    RBrace,
    At,
    Bang,
    Hash,
    /// `:.`
    TroTrailing,
    /// `.:`
    TroLeading,
    /// `.:.`
    TroAll,
}

#[derive(Clone, Copy, Debug)]
enum TokK {
    Num { v: f64, raw: StrIx },
    Str(StrIx),
    Err(ErrorCode),
    Ident(u32, u32),
    Func(u32, u32),
    Ref(NodeId),
    Sheet(u32),
    Table(NodeId),
    Op(Op),
    Ws,
    Eof,
}

#[derive(Clone, Copy, Debug)]
struct Tok {
    k: TokK,
    p: u32,
}

/// `BIN_PREC` of an operator token (the binary/postfix operators).
const fn bin_prec(op: Op) -> Option<u8> {
    Some(match op {
        Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => 1,
        Op::Amp => 2,
        Op::Plus | Op::Minus => 3,
        Op::Star | Op::Slash => 4,
        Op::Caret => 5,
        Op::Percent => 6,
        Op::Comma => 8,
        Op::Colon | Op::TroTrailing | Op::TroLeading | Op::TroAll => 10,
        _ => return None,
    })
}

const PREC_ISECT: u8 = 9;
const PREC_UNARY: u8 = 7;
const PREC_SINGLE: u8 = 8;

/// The operator found by `peekOp`.
#[derive(Clone, Copy, Debug)]
enum PeekOp {
    Tok(Op),
    Isect,
}

/// What to do with the next operand (the continuations of the TS recursion).
#[derive(Clone, Copy, Debug)]
enum Frame {
    /// `expr(min, inParens)`: run the operator loop on the delivered operand.
    Expr { min: u8, inp: bool },
    /// The right operand of `left op …` arrived.
    BinRight { op: PeekOp, left: NodeId, ldepth: u32, min: u8, inp: bool },
    /// `-x`/`+x`.
    Unary(UnaryOp),
    /// `@x`.
    Single,
    /// `( … )`.
    Paren { p: u32 },
    /// An argument of a list (`start` = its first entry on the argument stack).
    Arg { kind: ArgsKind, start: u32 },
}

/// What an argument list builds when it closes.
#[derive(Clone, Copy, Debug)]
enum ArgsKind {
    /// `FUNC(…)` (the fn node to fill).
    Fn { node: NodeId },
    /// `_xlpm.f(…)`: a call of the parameter `name`.
    Param { name: NodeId },
    /// `Plan1!FUNC(…)`: a call of the sheet-scoped name.
    SheetFn { name: NodeId },
    /// `(…)(…)` after a paren/call/fn.
    Postfix { callee: NodeId, cdepth: u32 },
}

enum Step {
    /// Parse an operand (`prefix(inParens)`).
    Operand { inp: bool },
    /// An operand to deliver to the top frame.
    Deliver(NodeId, u32),
    /// Run `postfix` on a node, then deliver it.
    Postfix(NodeId, u32),
    /// Parse the next argument of the list at the top of the argument stack (`args()` loop body).
    NextArg { kind: ArgsKind, start: u32 },
}

/// Reusable buffers of the parser (tokens, continuation frames, the argument stack).
#[derive(Debug, Default)]
pub struct FormulaParser {
    toks: Vec<Tok>,
    frames: Vec<Frame>,
    args: Vec<(NodeId, u32)>,
    units: Vec<u16>,
    scratch: Vec<u16>,
    arr: Vec<Scalar>,
}

/// `parseFormula(text, o)` with fresh buffers.
pub fn parse_formula(ast: &mut Ast, text: JsStr<'_>, o: &ParseOptions<'_>) -> Result<NodeId, SyntaxError> {
    FormulaParser::default().parse(ast, text, o)
}

impl FormulaParser {
    /// A parser with empty buffers.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `parseFormula(text, o)`: the root node in `ast`; on a syntax error the arena is rolled back.
    pub fn parse(&mut self, ast: &mut Ast, text: JsStr<'_>, o: &ParseOptions<'_>) -> Result<NodeId, SyntaxError> {
        let mark = ast.mark();
        let mut units = core::mem::take(&mut self.units);
        units.clear();
        units.extend(text.units());
        let r = self.run(ast, &units, o);
        self.units = units;
        if r.is_err() {
            ast.rollback(mark);
        }
        r
    }

    fn run(&mut self, ast: &mut Ast, s: &[u16], o: &ParseOptions<'_>) -> Result<NodeId, SyntaxError> {
        self.toks.clear();
        self.frames.clear();
        self.args.clear();
        let mut lx = Lexer { s, o, toks: &mut self.toks, ast, scratch: &mut self.scratch };
        lx.run()?;
        let mut p =
            Parser { s, toks: &self.toks, i: 0, ast, frames: &mut self.frames, args: &mut self.args, arr: &mut self.arr, host_col: o.col };
        p.parse_all()
    }
}

// ------------------------------------------------------------------------------------------------ lexer

struct Lexer<'a, 'o> {
    s: &'a [u16],
    o: &'a ParseOptions<'o>,
    toks: &'a mut Vec<Tok>,
    ast: &'a mut Ast,
    scratch: &'a mut Vec<u16>,
}

fn fail<T>(kind: SyntaxKind, p: usize) -> Result<T, SyntaxError> {
    Err(SyntaxError { kind, pos: p as u32 })
}

/// `SHEET_RE` at `i`: `(end, ext digits, name1, name2)` with the ranges of the groups.
type SheetMatch = (usize, Option<(usize, usize)>, Option<(usize, usize)>);

fn sheet_re(s: &[u16], i: usize) -> Option<SheetMatch> {
    let mut j = i;
    let mut ext = None;
    if s.get(j) == Some(&0x5b) {
        let n = digit_len(s, j + 1);
        if n > 0 && s.get(j + 1 + n) == Some(&0x5d) {
            ext = Some((j + 1, j + 1 + n));
            j += n + 2;
        }
    }
    let n1 = j;
    while j < s.len() && sheet_char(s[j]) {
        j += 1;
    }
    if j > n1 {
        let name1 = Some((n1, j));
        if s.get(j) == Some(&0x3a) {
            let mut k = j + 1;
            while k < s.len() && sheet_char(s[k]) {
                k += 1;
            }
            if k > j + 1 && s.get(k) == Some(&0x21) {
                return Some((k + 1, ext, name1));
            }
            return None;
        }
        if s.get(j) == Some(&0x21) {
            return Some((j + 1, ext, name1));
        }
        return None;
    }
    if s.get(j) == Some(&0x21) {
        return Some((j + 1, ext, None));
    }
    None
}

impl Lexer<'_, '_> {
    fn push(&mut self, k: TokK, p: usize) {
        self.toks.push(Tok { k, p: p as u32 });
    }

    fn run(&mut self) -> Result<(), SyntaxError> {
        let mut i = 0;
        while i < self.s.len() {
            i = self.next(i)?;
        }
        self.push(TokK::Eof, self.s.len());
        Ok(())
    }

    /// `sheetSpecOf(content)`.
    fn sheet_spec(&mut self, content: &[u16]) -> u32 {
        let mut c = content;
        let mut ext = None;
        if c.first() == Some(&0x5b) {
            let n = digit_len(c, 1);
            if n > 0 && c.get(1 + n) == Some(&0x5d) {
                ext = Some(digits_value(&c[1..=n]));
                c = &c[n + 2..];
            }
        }
        let spec = match c.iter().position(|&u| u == 0x3a) {
            None => SheetSpec { sheet: self.ast.intern_units(c), sheet2: None, ext },
            Some(k) => SheetSpec { sheet: self.ast.intern_units(&c[..k]), sheet2: Some(self.ast.intern_units(&c[k + 1..])), ext },
        };
        self.ast.push_spec(spec)
    }

    fn row_part(&mut self, r: f64, abs: bool) -> Part {
        let n = if abs { r } else { r - f64::from(self.o.row) };
        self.ast.pack_part(RefPart { n, abs })
    }

    fn col_part(&mut self, c: f64, abs: bool) -> Part {
        let n = if abs { c } else { c - f64::from(self.o.col) };
        self.ast.pack_part(RefPart { n, abs })
    }

    fn next(&mut self, i: usize) -> Result<usize, SyntaxError> {
        let s = self.s;
        let ch = s[i];
        if lex_space(ch) {
            let mut j = i + 1;
            while j < s.len() && lex_space(s[j]) {
                j += 1;
            }
            self.push(TokK::Ws, i);
            return Ok(j);
        }
        if ch == 0x22 || ch == 0x27 {
            // "text" / 'sheet'!: the quote doubled inside
            let mut j = i + 1;
            self.scratch.clear();
            loop {
                if j >= s.len() {
                    return fail(if ch == 0x22 { SyntaxKind::UnterminatedText } else { SyntaxKind::UnterminatedSheet }, i);
                }
                if s[j] == ch {
                    if s.get(j + 1) == Some(&ch) {
                        self.scratch.push(ch);
                        j += 2;
                        continue;
                    }
                    j += 1;
                    break;
                }
                self.scratch.push(s[j]);
                j += 1;
            }
            if ch == 0x22 {
                let v = self.ast.intern_units(&core::mem::take(self.scratch));
                self.push(TokK::Str(v), i);
                return Ok(j);
            }
            if s.get(j) != Some(&0x21) {
                return fail(SyntaxKind::SheetWithoutBang, j);
            }
            let content = core::mem::take(self.scratch);
            let spec = self.sheet_spec(&content);
            *self.scratch = content;
            self.push(TokK::Sheet(spec), i);
            return Ok(j + 1);
        }
        if ch == 0x23 {
            // `s.slice(i, i + 14).toUpperCase()` starting with an error code
            let end = (i + 14).min(s.len());
            let up = uo_text::case::to_uppercase(&s[i..end]);
            for (code, e) in ERROR_CODES {
                if up.len() >= code.len() && code.bytes().zip(up.iter()).all(|(a, &b)| u16::from(a) == b) {
                    self.push(TokK::Err(e), i);
                    return Ok(i + code.len());
                }
            }
            self.push(TokK::Op(Op::Hash), i);
            return Ok(i + 1);
        }
        if ch == 0x5b {
            if let Some((end, Some(_), _)) = sheet_re(s, i) {
                let spec = self.sheet_spec(&s[i..end - 1]);
                self.push(TokK::Sheet(spec), i);
                return Ok(end);
            }
            return self.table(Ast::EMPTY, i);
        }
        let next_digit = s.get(i + 1).is_some_and(|&u| is_digit(u));
        if is_digit(ch) || (ch == 0x2e && next_digit) || (ch == 0x24 && next_digit) {
            if !self.o.r1c1
                && let Some(end) = self.rows_ref(i)
            {
                return Ok(end);
            }
            if ch == 0x24 {
                return fail(SyntaxKind::InvalidRef, i);
            }
            // NUM_RE: (\d+\.?\d*|\.\d+)([eE][+-]?\d+)?
            let mut j = i + digit_len(s, i);
            if j > i {
                if s.get(j) == Some(&0x2e) {
                    j += 1;
                    j += digit_len(s, j);
                }
            } else {
                j = i + 1 + digit_len(s, i + 1);
            }
            if matches!(s.get(j), Some(&0x45 | &0x65)) {
                let mut k = j + 1;
                if matches!(s.get(k), Some(&0x2b | &0x2d)) {
                    k += 1;
                }
                let n = digit_len(s, k);
                if n > 0 {
                    j = k + n;
                }
            }
            let Some((v, truncated)) = number_literal_units(&s[i..j]) else {
                return fail(SyntaxKind::NumberOutOfRange, i);
            };
            let raw = if truncated { Ast::EMPTY } else { self.ast.intern_units(&s[i..j]) };
            self.push(TokK::Num { v, raw }, i);
            return Ok(j);
        }
        if ch == 0x24 || ident_start(ch) {
            if ch != 0x24
                && let Some((end, _, Some(_))) = sheet_re(s, i)
            {
                let spec = self.sheet_spec(&s[i..end - 1]);
                self.push(TokK::Sheet(spec), i);
                return Ok(end);
            }
            let r = if self.o.r1c1 { self.r1c1_ref(i) } else { self.a1_ref(i) };
            if let Some((node, end)) = r {
                self.push(TokK::Ref(node), i);
                return Ok(end);
            }
            if ch == 0x24 {
                return fail(SyntaxKind::InvalidRef, i);
            }
            let mut j = i + 1;
            while j < s.len() && ident_char(s[j]) {
                j += 1;
            }
            if s.get(j) == Some(&0x28) {
                self.push(TokK::Func(i as u32, j as u32), i);
                return Ok(j + 1);
            }
            if s.get(j) == Some(&0x5b) {
                let name = self.ast.intern_units(&s[i..j]);
                return self.table(name, j);
            }
            self.push(TokK::Ident(i as u32, j as u32), i);
            return Ok(j);
        }
        let two = |a: u16, b: u16| ch == a && s.get(i + 1) == Some(&b);
        for (a, b, op) in [(b'<', b'>', Op::Ne), (b'<', b'=', Op::Le), (b'>', b'=', Op::Ge)] {
            if two(u16::from(a), u16::from(b)) {
                self.push(TokK::Op(op), i);
                return Ok(i + 2);
            }
        }
        if ch == 0x2e && s.get(i + 1) == Some(&0x3a) && s.get(i + 2) == Some(&0x2e) {
            self.push(TokK::Op(Op::TroAll), i);
            return Ok(i + 3);
        }
        if two(0x2e, 0x3a) {
            self.push(TokK::Op(Op::TroLeading), i);
            return Ok(i + 2);
        }
        if two(0x3a, 0x2e) {
            self.push(TokK::Op(Op::TroTrailing), i);
            return Ok(i + 2);
        }
        let op = match ch {
            0x2b => Op::Plus,
            0x2d => Op::Minus,
            0x2a => Op::Star,
            0x2f => Op::Slash,
            0x5e => Op::Caret,
            0x26 => Op::Amp,
            0x3d => Op::Eq,
            0x3c => Op::Lt,
            0x3e => Op::Gt,
            0x25 => Op::Percent,
            0x3a => Op::Colon,
            0x2c => Op::Comma,
            0x3b => Op::Semi,
            0x28 => Op::LParen,
            0x29 => Op::RParen,
            0x7b => Op::LBrace,
            0x7d => Op::RBrace,
            0x40 => Op::At,
            0x21 => Op::Bang,
            _ => return fail(SyntaxKind::UnexpectedChar, i),
        };
        self.push(TokK::Op(op), i);
        Ok(i + 1)
    }

    /// `ROWS_RE`: `(\$?)(\d{1,7}):(\$?)(\d{1,7})(?![\p{L}\p{N}_.\\?(])` → a whole-rows area.
    fn rows_ref(&mut self, i: usize) -> Option<usize> {
        let s = self.s;
        let mut j = i;
        let a1 = s.get(j) == Some(&0x24);
        j += usize::from(a1);
        let n1 = digit_len(s, j);
        if n1 == 0 || n1 > 7 || s.get(j + n1) != Some(&0x3a) {
            return None;
        }
        let d1 = &s[j..j + n1];
        j += n1 + 1;
        let a2 = s.get(j) == Some(&0x24);
        j += usize::from(a2);
        let n2 = digit_len(s, j);
        if n2 == 0 || n2 > 7 {
            return None;
        }
        let d2 = &s[j..j + n2];
        j += n2;
        if !lookahead_ok(s, j, b"_\\?(", false, true) {
            return None;
        }
        let (r1, r2) = (digits_value(d1) - 1.0, digits_value(d2) - 1.0);
        let max = f64::from(MAX_ROW);
        if !(r1 <= max && r2 <= max && r1 >= 0.0 && r2 >= 0.0) {
            return None;
        }
        let r1 = self.row_part(r1, a1);
        let r2 = self.row_part(r2, a2);
        let node = self.ast.push(Node::Area { sheet: 0, r1, c1: Part::NONE, r2, c2: Part::NONE }, None);
        self.push(TokK::Ref(node), i);
        Some(j)
    }

    fn a1_ref(&mut self, i: usize) -> Option<(NodeId, usize)> {
        let s = self.s;
        // COLS_RE: (\$?)([A-Za-z]{1,3}):(\$?)([A-Za-z]{1,3})(?![\p{L}\p{N}_\\?(!]|\.(?!:))
        let letters = |j: usize| s.get(j..).map_or(0, |r| r.iter().take_while(|&&u| is_alpha(u)).count());
        let mut j = i;
        let a1 = s.get(j) == Some(&0x24);
        j += usize::from(a1);
        let l1 = letters(j);
        if (1..=3).contains(&l1) && s.get(j + l1) == Some(&0x3a) {
            let mut k = j + l1 + 1;
            let a2 = s.get(k) == Some(&0x24);
            k += usize::from(a2);
            let l2 = letters(k);
            if (1..=3).contains(&l2)
                && lookahead_ok(s, k + l2, b"_\\?(!", true, false)
                && let (Some(c1), Some(c2)) = (col_from_letters(&s[j..j + l1]), col_from_letters(&s[k..k + l2]))
            {
                let c1 = self.col_part(f64::from(c1), a1);
                let c2 = self.col_part(f64::from(c2), a2);
                let node = self.ast.push(Node::Area { sheet: 0, r1: Part::NONE, c1, r2: Part::NONE, c2 }, None);
                return Some((node, k + l2));
            }
        }
        // CELL_RE: (\$?)([A-Za-z]{1,3})(\$?)(\d{1,7})(?![\p{L}\p{N}_\\?([!]|\.(?!:))
        if (1..=3).contains(&l1) {
            let mut k = j + l1;
            let ra = s.get(k) == Some(&0x24);
            k += usize::from(ra);
            let n = digit_len(s, k);
            if (1..=7).contains(&n) && lookahead_ok(s, k + n, b"_\\?([!", true, false) {
                let col = col_from_letters(&s[j..j + l1]);
                let row = digits_value(&s[k..k + n]) - 1.0;
                if let Some(col) = col
                    && row >= 0.0
                    && row <= f64::from(MAX_ROW)
                {
                    let row = self.row_part(row, ra);
                    let col = self.col_part(f64::from(col), a1);
                    let node = self.ast.push(Node::Cell { sheet: 0, row, col }, None);
                    return Some((node, k + n));
                }
            }
        }
        None
    }

    /// `r1c1Part(x, max)`: `x` absent → offset 0; `[n]` → offset; digits → absolute (1-based, range checked).
    fn r1c1_part(&mut self, x: Option<(usize, usize)>, max: u32) -> Option<Part> {
        let Some((a, b)) = x else {
            return Some(self.ast.pack_part(RefPart { n: 0.0, abs: false }));
        };
        let t = &self.s[a..b];
        if t[0] == 0x5b {
            // +x.slice(1, -1): an optional '-' and digits
            let inner = &t[1..t.len() - 1];
            let (neg, d) = if inner[0] == 0x2d { (true, &inner[1..]) } else { (false, inner) };
            let v = digits_value(d);
            return Some(self.ast.pack_part(RefPart { n: if neg { -v } else { v }, abs: false }));
        }
        let n = digits_value(t) - 1.0;
        (n >= 0.0 && n <= f64::from(max)).then(|| self.ast.pack_part(RefPart { n, abs: true }))
    }

    fn r1c1_ref(&mut self, i: usize) -> Option<(NodeId, usize)> {
        let s = self.s;
        let (row_set, col_set) = letter_sets(self.o.letters);
        let is_r = |j: usize| s.get(j).is_some_and(|u| row_set.contains(u));
        let is_c = |j: usize| s.get(j).is_some_and(|u| col_set.contains(u));
        let end_ok = |e: usize| lookahead_ok(s, e, br"_\?([!", false, true);
        // the alternatives of an optional N (`\[-?\d+\]|\d+`) at j, in the order the RegExp tries them: present
        // (its end), then absent; a shorter digit run is always followed by a digit, which no continuation takes
        let cands = |j: usize| -> [Option<Option<(usize, usize)>>; 2] {
            let present = if s.get(j) == Some(&0x5b) {
                let k = j + 1 + usize::from(s.get(j + 1) == Some(&0x2d));
                let n = digit_len(s, k);
                (n > 0 && s.get(k + n) == Some(&0x5d)).then_some((j, k + n + 1))
            } else {
                let n = digit_len(s, j);
                (n > 0).then_some((j, j + n))
            };
            [present.map(Some), Some(None)]
        };
        let after = |g: Option<(usize, usize)>, j: usize| g.map_or(j, |(_, e)| e);
        // cell: (R)N?(C)N?END
        let mut cell = None;
        if is_r(i) {
            'outer: for rn in cands(i + 1).into_iter().flatten() {
                let cpos = after(rn, i + 1);
                if !is_c(cpos) {
                    continue;
                }
                for cn in cands(cpos + 1).into_iter().flatten() {
                    let e = after(cn, cpos + 1);
                    if end_ok(e) {
                        cell = Some((rn, cn, e));
                        break 'outer;
                    }
                }
            }
        }
        if let Some((rn, cn, end)) = cell {
            let row = self.r1c1_part(rn, MAX_ROW);
            let col = self.r1c1_part(cn, MAX_COL);
            if let (Some(row), Some(col)) = (row, col) {
                return Some((self.ast.push(Node::Cell { sheet: 0, row, col }, None), end));
            }
        }
        // rows: (R)N?(?:(:)(R)N?)?END ; cols likewise with C
        for rows in [true, false] {
            let is_x = |j: usize| if rows { is_r(j) } else { is_c(j) };
            if !is_x(i) {
                continue;
            }
            let mut m = None;
            'outer2: for n1 in cands(i + 1).into_iter().flatten() {
                let p = after(n1, i + 1);
                if s.get(p) == Some(&0x3a) && is_x(p + 1) {
                    for n2 in cands(p + 2).into_iter().flatten() {
                        let e = after(n2, p + 2);
                        if end_ok(e) {
                            m = Some((n1, Some(n2), e));
                            break 'outer2;
                        }
                    }
                }
                if end_ok(p) {
                    m = Some((n1, None, p));
                    break;
                }
            }
            let Some((n1, n2, end)) = m else { continue };
            let max = if rows { MAX_ROW } else { MAX_COL };
            let p1 = self.r1c1_part(n1, max);
            let p2 = match n2 {
                Some(g) => self.r1c1_part(g, max),
                None => p1,
            };
            if let (Some(p1), Some(p2)) = (p1, p2) {
                let n = if rows {
                    Node::Area { sheet: 0, r1: p1, c1: Part::NONE, r2: p2, c2: Part::NONE }
                } else {
                    Node::Area { sheet: 0, r1: Part::NONE, c1: p1, r2: Part::NONE, c2: p2 }
                };
                return Some((self.ast.push(n, None), end));
            }
        }
        None
    }

    /// A structured reference from the `[` at `i`.
    fn table(&mut self, name: StrIx, i: usize) -> Result<usize, SyntaxError> {
        let s = self.s;
        let mut j = i;
        let mut depth = 0i64;
        while j < s.len() {
            let ch = s[j];
            if ch == 0x27 {
                j += 2;
                continue;
            }
            if ch == 0x5b {
                depth += 1;
            } else if ch == 0x5d {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            j += 1;
        }
        if depth != 0 || j >= s.len() {
            return fail(SyntaxKind::UnclosedTable, i);
        }
        let node = parse_table_spec(self.ast, name, &s[i + 1..j], i)?;
        self.push(TokK::Table(node), i);
        Ok(j + 1)
    }
}

/// The R1C1 letter sets: `[${row.toUpperCase()}${row.toLowerCase()}]` (code units of both cases; the
/// standard pair when the letters are R and C).
fn letter_sets(l: Option<(JsStr<'_>, JsStr<'_>)>) -> (Vec<u16>, Vec<u16>) {
    let set = |x: JsStr<'_>| -> Vec<u16> {
        let u = x.to_utf16();
        let mut v = uo_text::case::to_uppercase(&u).into_owned();
        v.extend_from_slice(&uo_text::case::to_lowercase(&u));
        v
    };
    match l {
        Some((r, c)) => {
            let (ru, cu) =
                (uo_text::case::to_uppercase(&r.to_utf16()).into_owned(), uo_text::case::to_uppercase(&c.to_utf16()).into_owned());
            if ru == [u16::from(b'R')] && cu == [u16::from(b'C')] { (vec![0x52, 0x72], vec![0x43, 0x63]) } else { (set(r), set(c)) }
        }
        None => (vec![0x52, 0x72], vec![0x43, 0x63]),
    }
}

/// `ITEMS[p.trim().toUpperCase()]`.
fn item_of(p: &[u16]) -> Option<TableItem> {
    let t = js_trim(p);
    let up = uo_text::case::to_uppercase(t);
    let is = |w: &str| up.len() == w.len() && w.bytes().zip(up.iter()).all(|(a, &b)| u16::from(a) == b);
    Some(if is("#ALL") {
        TableItem::All
    } else if is("#DATA") {
        TableItem::Data
    } else if is("#HEADERS") {
        TableItem::Headers
    } else if is("#TOTALS") {
        TableItem::Totals
    } else if is("#THIS ROW") {
        TableItem::ThisRow
    } else {
        return None;
    })
}

/// `unescapeCol`: `'x` → `x`.
fn unescape_col(s: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == 0x27 && i + 1 < s.len() {
            out.push(s[i + 1]);
            i += 2;
            continue;
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// A group of `groups()`: a `:` or the inside of a `[…]`.
enum Group<'a> {
    Colon,
    Part(&'a [u16]),
}

fn groups(s: &[u16]) -> Option<Vec<Group<'_>>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        while i < s.len() && matches!(s[i], 0x20 | 0x2c | 0x3a) {
            if s[i] == 0x3a {
                out.push(Group::Colon);
            }
            i += 1;
        }
        if i >= s.len() {
            break;
        }
        if s[i] != 0x5b {
            return None;
        }
        let mut j = i + 1;
        let mut depth = 1i64;
        while j < s.len() {
            if s[j] == 0x27 {
                j += 2;
                continue;
            }
            if s[j] == 0x5b {
                depth += 1;
            } else if s[j] == 0x5d {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            j += 1;
        }
        if depth != 0 {
            return None;
        }
        out.push(Group::Part(&s[i + 1..j.min(s.len())]));
        i = j + 1;
    }
    Some(out)
}

/// `parseTableSpec(table, inner, pos)`.
fn parse_table_spec(ast: &mut Ast, table: StrIx, inner: &[u16], pos: usize) -> Result<NodeId, SyntaxError> {
    let bad = || SyntaxError { kind: SyntaxKind::InvalidTable, pos: pos as u32 };
    let mut items: Vec<TableItem> = Vec::new();
    let mut col1: Option<Vec<u16>> = None;
    let mut col2: Option<Vec<u16>> = None;
    let mut add_cols = |parts: &[Group<'_>], items: &mut Vec<TableItem>| -> Result<(), SyntaxError> {
        for (k, g) in parts.iter().enumerate() {
            let Group::Part(p) = g else { continue };
            if let Some(kw) = item_of(p) {
                items.push(kw);
                continue;
            }
            let name = unescape_col(p);
            if col1.is_none() {
                col1 = Some(name);
            } else if k > 0 && matches!(parts[k - 1], Group::Colon) && col2.is_none() {
                col2 = Some(name);
            } else {
                return Err(bad());
            }
        }
        Ok(())
    };
    let t = js_trim(inner);
    if t.is_empty() {
        // Tabela[] = data
    } else if t[0] == 0x40 {
        items.push(TableItem::ThisRow);
        let rest = js_trim(&t[1..]);
        if rest.first() == Some(&0x5b) {
            let g = groups(rest).ok_or_else(bad)?;
            add_cols(&g, &mut items)?;
        } else if !rest.is_empty() {
            col1 = Some(unescape_col(rest));
        }
    } else if t[0] == 0x5b {
        let g = groups(t).ok_or_else(bad)?;
        add_cols(&g, &mut items)?;
    } else {
        // ITEMS[t.toUpperCase()] (t is trimmed already)
        match item_of(t) {
            Some(k) => items.push(k),
            None => col1 = Some(unescape_col(inner)),
        }
    }
    let items_at = ast.push_items(&items);
    let col1 = col1.map_or(0, |c| ast.intern_units(&c).0 + 1);
    let col2 = col2.map_or(0, |c| ast.intern_units(&c).0 + 1);
    Ok(ast.push(Node::Table { table, items: items_at, n_items: items.len() as u32, col1, col2 }, None))
}

// ------------------------------------------------------------------------------------------------ parser

struct Parser<'a> {
    s: &'a [u16],
    toks: &'a [Tok],
    i: usize,
    ast: &'a mut Ast,
    frames: &'a mut Vec<Frame>,
    args: &'a mut Vec<(NodeId, u32)>,
    arr: &'a mut Vec<Scalar>,
    host_col: u32,
}

/// The upper-cased text of an identifier and its prefix-stripped canonical function name.
fn upper_units(s: &[u16]) -> Vec<u16> {
    uo_text::case::to_uppercase(s).into_owned()
}

fn ascii_ci_prefix(s: &[u16], p: &[u8]) -> bool {
    s.len() >= p.len() && ascii_ci_eq(&s[..p.len()], &p.to_ascii_uppercase())
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Tok {
        self.toks[self.i]
    }

    fn take(&mut self) -> Tok {
        let t = self.toks[self.i];
        self.i += 1;
        t
    }

    fn skip_ws(&mut self) {
        while matches!(self.toks[self.i].k, TokK::Ws) {
            self.i += 1;
        }
    }

    fn is_op(t: Tok, op: Op) -> bool {
        matches!(t.k, TokK::Op(o) if o == op)
    }

    fn fail<T>(&self, kind: SyntaxKind, t: Tok) -> Result<T, SyntaxError> {
        Err(SyntaxError { kind, pos: t.p })
    }

    /// The source units of a token (identifiers are slices of the text).
    fn src(&self, a: u32, b: u32) -> &'a [u16] {
        let s = self.s;
        &s[a as usize..b as usize]
    }

    fn check_depth(&self, d: u32) -> Result<(), SyntaxError> {
        if d > MAX_AST_DEPTH {
            return self.fail(SyntaxKind::TooDeep, self.toks[self.i.min(self.toks.len() - 1)]);
        }
        Ok(())
    }

    /// A continuation (heap: each one consumed a token, so the stack is bounded by the text).
    fn push_frame(&mut self, f: Frame) -> Result<(), SyntaxError> {
        self.frames.push(f);
        Ok(())
    }

    fn parse_all(&mut self) -> Result<NodeId, SyntaxError> {
        self.skip_ws();
        if Self::is_op(self.peek(), Op::Eq) {
            self.i += 1;
            self.skip_ws();
        }
        self.push_frame(Frame::Expr { min: 0, inp: false })?;
        let mut step = Step::Operand { inp: false };
        loop {
            step = match step {
                Step::Operand { inp } => self.prefix(inp)?,
                Step::Postfix(n, d) => self.postfix(n, d)?,
                Step::NextArg { kind, start } => self.next_arg(kind, start)?,
                Step::Deliver(n, d) => {
                    self.check_depth(d)?;
                    let Some(f) = self.frames.pop() else {
                        self.skip_ws();
                        if !matches!(self.peek().k, TokK::Eof) {
                            return self.fail(SyntaxKind::Trailing, self.peek());
                        }
                        return Ok(n);
                    };
                    self.deliver(f, n, d)?
                }
            };
        }
    }

    /// `peekOp(inParens)`.
    fn peek_op(&self, inp: bool) -> Option<(PeekOp, usize)> {
        let mut j = self.i;
        let mut ws = false;
        while matches!(self.toks[j].k, TokK::Ws) {
            j += 1;
            ws = true;
        }
        let t = self.toks[j];
        match t.k {
            TokK::Op(op) => {
                if op == Op::Comma {
                    return inp.then_some((PeekOp::Tok(op), j + 1));
                }
                if bin_prec(op).is_some() {
                    return Some((PeekOp::Tok(op), j + 1));
                }
                if ws && (op == Op::LParen || op == Op::At) {
                    return Some((PeekOp::Isect, j));
                }
                None
            }
            TokK::Ref(_) | TokK::Sheet(_) | TokK::Ident(..) | TokK::Func(..) | TokK::Table(_) if ws => Some((PeekOp::Isect, j)),
            _ => None,
        }
    }

    fn deliver(&mut self, f: Frame, n: NodeId, d: u32) -> Result<Step, SyntaxError> {
        Ok(match f {
            Frame::Expr { min, inp } => {
                let Some((op, next)) = self.peek_op(inp) else { return Ok(Step::Deliver(n, d)) };
                let prec = match op {
                    PeekOp::Isect => PREC_ISECT,
                    PeekOp::Tok(o) => bin_prec(o).unwrap_or(0),
                };
                if prec < min {
                    return Ok(Step::Deliver(n, d));
                }
                self.i = next;
                if matches!(op, PeekOp::Tok(Op::Percent)) {
                    let u = self.ast.push(Node::Unary { op: UnaryOp::Percent, e: n }, None);
                    self.push_frame(Frame::Expr { min, inp })?;
                    return Ok(Step::Deliver(u, d));
                }
                self.push_frame(Frame::BinRight { op, left: n, ldepth: d, min, inp })?;
                self.push_frame(Frame::Expr { min: prec + 1, inp })?;
                Step::Operand { inp }
            }
            Frame::BinRight { op, left, ldepth, min, inp } => {
                let (node, nd) = match op {
                    PeekOp::Tok(o @ (Op::TroTrailing | Op::TroLeading | Op::TroAll)) => {
                        let (b, bd) = self.make_bin(BinOp::Range, left, n, ldepth, d);
                        let name = match o {
                            Op::TroTrailing => "_TRO_TRAILING",
                            Op::TroLeading => "_TRO_LEADING",
                            _ => "_TRO_ALL",
                        };
                        (self.fn_node(name, Some("_xlfn."), &[b], None), bd + 1)
                    }
                    _ => {
                        let bop = match op {
                            PeekOp::Isect => BinOp::Isect,
                            PeekOp::Tok(o) => tok_bin(o),
                        };
                        self.make_bin(bop, left, n, ldepth, d)
                    }
                };
                self.push_frame(Frame::Expr { min, inp })?;
                Step::Deliver(node, nd)
            }
            // operators cost no level: the evaluator runs them on its work stack, the printer on its task stack
            Frame::Unary(op) => Step::Deliver(self.ast.push(Node::Unary { op, e: n }, None), d),
            Frame::Single => Step::Deliver(self.fn_node("SINGLE", Some("_xlfn."), &[n], None), d + 1),
            Frame::Paren { p } => {
                self.skip_ws();
                let t = self.take();
                if !Self::is_op(t, Op::RParen) {
                    // the TS peeks past the end when the `(` is never closed (a TypeError there, A2-4)
                    return match self.toks.get(self.i) {
                        Some(&at) => self.fail(SyntaxKind::MissingParen, at),
                        None => Err(SyntaxError { kind: SyntaxKind::UnclosedParenAtEnd, pos: self.s.len() as u32 }),
                    };
                }
                // parentheses cost no level: the evaluator and the printer skip them in a loop
                Step::Postfix(self.ast.push(Node::Paren(n), Some(p)), d)
            }
            Frame::Arg { kind, start } => {
                self.args.push((n, d));
                self.after_arg(kind, start)?
            }
        })
    }

    /// After an argument: `skipWs; sep = take(); ')' → done; ',' → next; else fail`.
    fn after_arg(&mut self, kind: ArgsKind, start: u32) -> Result<Step, SyntaxError> {
        self.skip_ws();
        let sep = self.take();
        if Self::is_op(sep, Op::RParen) {
            return self.finish_args(kind, start);
        }
        if !Self::is_op(sep, Op::Comma) {
            return self.fail(SyntaxKind::ExpectedSeparator, sep);
        }
        Ok(Step::NextArg { kind, start })
    }

    /// The `args()` loop body: an omitted argument or an expression.
    fn next_arg(&mut self, kind: ArgsKind, start: u32) -> Result<Step, SyntaxError> {
        self.skip_ws();
        let t = self.peek();
        if Self::is_op(t, Op::Comma) || Self::is_op(t, Op::RParen) {
            let m = self.ast.push(Node::Missing, None);
            self.args.push((m, 1));
            return self.after_arg(kind, start);
        }
        self.push_frame(Frame::Arg { kind, start })?;
        self.push_frame(Frame::Expr { min: 0, inp: false })?;
        Ok(Step::Operand { inp: false })
    }

    /// `args()` from just after the `(`.
    fn begin_args(&mut self, kind: ArgsKind) -> Result<Step, SyntaxError> {
        let start = self.args.len() as u32;
        self.skip_ws();
        if Self::is_op(self.peek(), Op::RParen) {
            self.i += 1;
            return self.finish_args(kind, start);
        }
        Ok(Step::NextArg { kind, start })
    }

    fn finish_args(&mut self, kind: ArgsKind, start: u32) -> Result<Step, SyntaxError> {
        let list: Vec<NodeId> = self.args[start as usize..].iter().map(|&(n, _)| n).collect();
        let ad = self.args[start as usize..].iter().map(|&(_, d)| d).max().unwrap_or(0);
        self.args.truncate(start as usize);
        let span = self.ast.push_kids(&list);
        Ok(match kind {
            ArgsKind::Fn { node } => {
                if let Node::Fn { name, prefix, sym, .. } = *self.ast.node(node) {
                    self.ast.set_node(node, Node::Fn { name, prefix, args: span, sym });
                }
                Step::Postfix(node, ad + 1)
            }
            ArgsKind::Param { name } | ArgsKind::SheetFn { name } => {
                Step::Postfix(self.ast.push(Node::Call { f: name, args: span }, None), ad.max(1) + 1)
            }
            ArgsKind::Postfix { callee, cdepth } => {
                Step::Postfix(self.ast.push(Node::Call { f: callee, args: span }, None), ad.max(cdepth) + 1)
            }
        })
    }

    /// `postfix(n)`: `#` (spill) and `(…)` (LAMBDA call) after an operand.
    fn postfix(&mut self, n: NodeId, d: u32) -> Result<Step, SyntaxError> {
        let t = self.peek();
        let kind = self.ast.node(n);
        if Self::is_op(t, Op::Hash)
            && matches!(kind, Node::Cell { .. } | Node::Name { .. } | Node::Area { .. } | Node::Fn { .. } | Node::Paren(_))
        {
            self.i += 1;
            let a = self.fn_node("ANCHORARRAY", Some("_xlfn."), &[n], None);
            self.check_depth(d + 1)?;
            return Ok(Step::Postfix(a, d + 1));
        }
        if Self::is_op(t, Op::LParen) && matches!(kind, Node::Paren(_) | Node::Call { .. } | Node::Fn { .. }) {
            self.i += 1;
            return self.begin_args(ArgsKind::Postfix { callee: n, cdepth: d });
        }
        Ok(Step::Deliver(n, d))
    }

    fn fn_node(&mut self, name: &str, prefix: Option<&str>, args: &[NodeId], pos: Option<u32>) -> NodeId {
        let sym = self.ast.intern(JsStr::Latin1(name.as_bytes()));
        let fname = crate::fnreg::find(JsStr::Latin1(name.as_bytes())).map_or(FnName::Other(sym), FnName::Known);
        let prefix = prefix.map_or(0, |p| self.ast.intern(JsStr::Latin1(p.as_bytes())).0 + 1);
        let span = self.ast.push_kids(args);
        self.ast.push(Node::Fn { name: fname, prefix, args: span, sym }, pos)
    }

    /// `fnName(raw)` → a fn node (args filled later).
    fn fn_from_raw(&mut self, raw: &[u16], pos: Option<u32>) -> NodeId {
        let mut i = 0;
        loop {
            let rest = &raw[i..];
            let k = [&b"_xlfn."[..], b"_xlws.", b"_xludf.", b"_xll."].iter().find(|p| ascii_ci_prefix(rest, p)).map(|p| p.len());
            match k {
                Some(k) => i += k,
                None => break,
            }
        }
        let up = upper_units(&raw[i..]);
        let sym = self.ast.intern_units(&up);
        let name = crate::fnreg::find(JsStr::Utf16(&up)).map_or(FnName::Other(sym), FnName::Known);
        let prefix = if i > 0 { self.ast.intern_units(&raw[..i]).0 + 1 } else { 0 };
        self.ast.push(Node::Fn { name, prefix, args: Span::default(), sym }, pos)
    }

    fn name_node(&mut self, name: &[u16], sheet: u32, flags: u8, pos: Option<u32>) -> NodeId {
        let n = self.ast.intern_units(name);
        let sym = self.ast.intern_upper(self.ast.str(n).to_owned_js().as_js());
        self.ast.push(Node::Name { name: n, sym, sheet, flags }, pos)
    }

    fn with_sheet(&mut self, n: NodeId, sheet: u32) {
        match *self.ast.node(n) {
            Node::Cell { row, col, .. } => self.ast.set_node(n, Node::Cell { sheet, row, col }),
            Node::Area { r1, c1, r2, c2, .. } => self.ast.set_node(n, Node::Area { sheet, r1, c1, r2, c2 }),
            _ => {}
        }
    }

    /// `makeBin(op, a, b, hostCol)`: the node and its depth.
    fn make_bin(&mut self, op: BinOp, a: NodeId, b: NodeId, ad: u32, bd: u32) -> (NodeId, u32) {
        if op == BinOp::Range {
            let (na, nb) = (*self.ast.node(a), *self.ast.node(b));
            if let (Node::Cell { sheet: sa, row: ra, col: ca }, Node::Cell { sheet: sb, row: rb, col: cb }) = (na, nb)
                && (sb == 0 || self.same_sheet(sa, sb))
            {
                // A1:B2 (same sheet) is one area, in the left cell's slot (its position kept)
                self.ast.set_node(a, Node::Area { sheet: sa, r1: ra, c1: ca, r2: rb, c2: cb });
                return (a, 1);
            }
            if let (Node::Name { name: an, sheet: sa, flags: fa, .. }, Node::Name { name: bn, sheet: sb, flags: fb, .. }) = (na, nb)
                && fa & name_flags::PARAM == 0
                && fb & name_flags::PARAM == 0
                && sb == 0
            {
                let (au, bu) = (self.ast.str(an).to_utf16(), self.ast.str(bn).to_utf16());
                let strip = |x: &[u16]| -> Vec<u16> { x.iter().copied().filter(|&u| u != 0x24).collect() };
                let shape = |x: &[u16]| {
                    let l = &x[usize::from(x.first() == Some(&0x24))..];
                    (1..=3).contains(&l.len()) && l.iter().all(|&u| is_alpha(u))
                };
                if let (Some(c1), Some(c2)) = (col_from_letters(&strip(&au)), col_from_letters(&strip(&bu)))
                    && shape(&au)
                    && shape(&bu)
                {
                    let part = |me: &mut Self, nm: &[u16], c: u32| {
                        let p = if nm.first() == Some(&0x24) {
                            RefPart { n: f64::from(c), abs: true }
                        } else {
                            RefPart { n: f64::from(c) - f64::from(me.host_col), abs: false }
                        };
                        me.ast.pack_part(p)
                    };
                    let c1 = part(self, &au, c1);
                    let c2 = part(self, &bu, c2);
                    self.ast.set_node(a, Node::Area { sheet: sa, r1: Part::NONE, c1, r2: Part::NONE, c2 });
                    return (a, 1);
                }
            }
        }
        let pos = self.ast.pos(a);
        (self.ast.push(Node::Bin { op, a, b }, pos), ad.max(bd))
    }

    /// `sameSheet(a, b)` of two spec indices.
    fn same_sheet(&self, a: u32, b: u32) -> bool {
        match (self.ast.spec(a), self.ast.spec(b)) {
            (None, None) => true,
            (Some(x), Some(y)) => {
                let up = |s: StrIx| uo_text::case::to_uppercase(&self.ast.str(s).to_utf16()).into_owned();
                let up2 = |s: Option<StrIx>| s.map_or_else(Vec::new, up);
                up(x.sheet) == up(y.sheet) && up2(x.sheet2) == up2(y.sheet2) && x.ext.unwrap_or(0.0) == y.ext.unwrap_or(0.0)
            }
            _ => false,
        }
    }

    /// `prefix(inParens)`.
    fn prefix(&mut self, inp: bool) -> Result<Step, SyntaxError> {
        self.skip_ws();
        let t = self.take();
        match t.k {
            TokK::Num { v, raw } => Ok(Step::Deliver(self.ast.push(Node::Num { v, raw: raw.0 }, None), 1)),
            TokK::Str(s) => Ok(Step::Deliver(self.ast.push(Node::Str(s), None), 1)),
            TokK::Err(e) => Ok(Step::Deliver(self.ast.push(Node::Err(e), None), 1)),
            TokK::Ref(n) | TokK::Table(n) => {
                self.ast.set_pos(n, Some(t.p));
                Ok(Step::Postfix(n, 1))
            }
            TokK::Sheet(spec) => {
                let n = self.take();
                match n.k {
                    TokK::Ref(r) => {
                        self.with_sheet(r, spec);
                        self.ast.set_pos(r, Some(t.p));
                        Ok(Step::Postfix(r, 1))
                    }
                    TokK::Err(ErrorCode::Ref) => Ok(Step::Deliver(self.ast.push(Node::RefErr { sheet: spec }, None), 1)),
                    TokK::Ident(a, b) => {
                        let nm = self.src_units(a, b);
                        let id = self.name_node(&nm, spec, 0, Some(t.p));
                        Ok(Step::Postfix(id, 1))
                    }
                    TokK::Func(a, b) => {
                        // `call({t:'fn', ...fnName(v)}, spec)`: a call of the name `f.name` on the sheet
                        let raw = self.src_units(a, b);
                        let f = self.fn_from_raw(&raw, None);
                        let Node::Fn { sym, .. } = *self.ast.node(f) else { unreachable!("fn node") };
                        let up = self.ast.str(sym).to_owned_js();
                        let nm = up.as_js().to_utf16();
                        let name = self.name_node(&nm, spec, 0, None);
                        self.begin_args(ArgsKind::SheetFn { name })
                    }
                    _ => self.fail(SyntaxKind::InvalidAfterSheet, n),
                }
            }
            TokK::Ident(a, b) => {
                let v = self.src_units(a, b);
                let u = upper_units(&v);
                let is_true = u == [0x54, 0x52, 0x55, 0x45];
                if is_true || u == [0x46, 0x41, 0x4c, 0x53, 0x45] {
                    return Ok(Step::Deliver(self.ast.push(Node::Bool(is_true), None), 1));
                }
                let starts = |p: &str| u.len() >= p.len() && p.bytes().zip(u.iter()).all(|(x, &y)| u16::from(x) == y);
                if starts("_XLPM.") {
                    let id = self.name_node(v.get(6..).unwrap_or(&[]), 0, name_flags::PARAM, None);
                    return Ok(Step::Deliver(id, 1));
                }
                if starts("_XLOP.") {
                    let id = self.name_node(v.get(6..).unwrap_or(&[]), 0, name_flags::PARAM | name_flags::OPTIONAL, None);
                    return Ok(Step::Deliver(id, 1));
                }
                if starts("_XLETA.") {
                    let up = upper_units(v.get(7..).unwrap_or(&[]));
                    let id = self.name_node(&up, 0, name_flags::ETA, None);
                    return Ok(Step::Deliver(id, 1));
                }
                let id = self.name_node(&v, 0, 0, Some(t.p));
                Ok(Step::Postfix(id, 1))
            }
            TokK::Func(a, b) => {
                let v = self.src_units(a, b);
                if ascii_ci_prefix(&v, b"_xlpm.") {
                    let name = self.name_node(&v[6..], 0, name_flags::PARAM, None);
                    return self.begin_args(ArgsKind::Param { name });
                }
                let f = self.fn_from_raw(&v, Some(t.p));
                self.begin_args(ArgsKind::Fn { node: f })
            }
            TokK::Op(op) => match op {
                Op::Minus | Op::Plus => {
                    self.push_frame(Frame::Unary(if op == Op::Minus { UnaryOp::Neg } else { UnaryOp::Plus }))?;
                    self.push_frame(Frame::Expr { min: PREC_UNARY, inp })?;
                    Ok(Step::Operand { inp })
                }
                Op::At => {
                    self.push_frame(Frame::Single)?;
                    self.push_frame(Frame::Expr { min: PREC_SINGLE, inp })?;
                    Ok(Step::Operand { inp })
                }
                Op::LParen => {
                    self.push_frame(Frame::Paren { p: t.p })?;
                    self.push_frame(Frame::Expr { min: 0, inp: true })?;
                    Ok(Step::Operand { inp: true })
                }
                Op::LBrace => self.array_const(),
                Op::Bang => {
                    let n = self.take();
                    let spec = self.ast.push_spec(SheetSpec { sheet: Ast::EMPTY, sheet2: None, ext: None });
                    match n.k {
                        TokK::Ref(r) => {
                            self.with_sheet(r, spec);
                            self.ast.set_pos(r, Some(t.p));
                            Ok(Step::Postfix(r, 1))
                        }
                        TokK::Ident(a, b) => {
                            let nm = self.src_units(a, b);
                            let id = self.name_node(&nm, spec, 0, Some(t.p));
                            Ok(Step::Postfix(id, 1))
                        }
                        _ => self.fail(SyntaxKind::InvalidAfterBang, n),
                    }
                }
                _ => self.fail(SyntaxKind::OperandExpected, t),
            },
            _ => self.fail(SyntaxKind::OperandExpected, t),
        }
    }

    fn src_units(&self, a: u32, b: u32) -> Vec<u16> {
        self.src(a, b).to_vec()
    }

    /// `arrayConst()` (scalars only: no recursion).
    fn array_const(&mut self) -> Result<Step, SyntaxError> {
        self.arr.clear();
        let mut widths: Vec<u32> = vec![0];
        loop {
            self.skip_ws();
            let mut t = self.take();
            let mut sign = 1.0f64;
            while let TokK::Op(o @ (Op::Minus | Op::Plus)) = t.k {
                if o == Op::Minus {
                    sign = -sign;
                }
                self.skip_ws();
                t = self.take();
            }
            let v = match t.k {
                TokK::Num { v, .. } => Scalar::Num(sign * v),
                _ if sign != 1.0 => return self.fail(SyntaxKind::InvalidArray, t),
                TokK::Str(s) => Scalar::Str(StrRef::Lit(s.0)),
                TokK::Err(e) => Scalar::Err(e),
                TokK::Ident(a, b) => {
                    let v = self.src_units(a, b);
                    if ascii_ci_eq(&v, b"TRUE") {
                        Scalar::Bool(true)
                    } else if ascii_ci_eq(&v, b"FALSE") {
                        Scalar::Bool(false)
                    } else {
                        return self.fail(SyntaxKind::InvalidArray, t);
                    }
                }
                _ => return self.fail(SyntaxKind::InvalidArray, t),
            };
            self.arr.push(v);
            if let Some(w) = widths.last_mut() {
                *w += 1;
            }
            self.skip_ws();
            let sep = self.take();
            if Self::is_op(sep, Op::RBrace) {
                break;
            }
            if Self::is_op(sep, Op::Comma) {
                continue;
            }
            if Self::is_op(sep, Op::Semi) {
                widths.push(0);
                continue;
            }
            return self.fail(SyntaxKind::InvalidArray, sep);
        }
        let w = widths[0];
        if widths.iter().any(|&x| x != w) {
            return self.fail(SyntaxKind::RaggedArray, self.peek());
        }
        let start = self.ast.push_array(self.arr);
        let id = self.ast.push(Node::Arr { start, rows: widths.len() as u32, cols: w }, None);
        Ok(Step::Deliver(id, 1))
    }
}

const fn tok_bin(o: Op) -> BinOp {
    match o {
        Op::Plus => BinOp::Add,
        Op::Minus => BinOp::Sub,
        Op::Star => BinOp::Mul,
        Op::Slash => BinOp::Div,
        Op::Caret => BinOp::Pow,
        Op::Amp => BinOp::Concat,
        Op::Eq => BinOp::Eq,
        Op::Ne => BinOp::Ne,
        Op::Lt => BinOp::Lt,
        Op::Le => BinOp::Le,
        Op::Gt => BinOp::Gt,
        Op::Ge => BinOp::Ge,
        Op::Comma => BinOp::Union,
        _ => BinOp::Range,
    }
}
