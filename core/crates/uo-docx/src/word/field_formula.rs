//! Formula fields (`= …`) recalculated by the layout (`docx-layout/src/field-formula.ts`):
//! arithmetic over the nested page fields (`{ = { NUMPAGES } - 1 }`), with numbers in the pt-BR
//! notation, `+ - * /`, parentheses and sign; the result formats (`\#` picture, `\*` general
//! formats, the default "up to two decimals"). Also the two docx-core helpers it imports from
//! `table-formula.ts` (`formatFormulaResult`, `formatPicture`), ported here because the layout
//! needs them (the session's table formulas reuse them in milestone 2).
//!
//! Limits (K11c, LQ13): the arithmetic parser is recursive (`-` and `(` nest) and the TS rethrows
//! the stack overflow (`RangeError`), failing the layout; here the depth is bounded by
//! `max_parse_depth` and passing it is [`FormulaError::Depth`].

use uo_jsnum::conv::is_integer;
use uo_jsnum::format::{to_fixed, to_js_string};
use uo_jsnum::math::{abs, max, pow, round, sign};
use uo_jsnum::text::is_js_space_unit;

use super::datefmt::{find_switch_picture, format_switches};
use super::numbering::{TextCap, format_number};
use crate::text::Utf16;

/// The value of a nested field (`FieldValue`, the paragraph's `ParaEnv.field`): name (upper
/// case) and instruction (` <inner trimmed> `); `None` = no value.
pub type FieldValue<'a> = dyn FnMut(&str, &[u16]) -> Option<Vec<u16>> + 'a;

/// Why a formula could not be evaluated where the TS throws (not where it keeps the cached
/// result: that is `Ok(None)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormulaError {
    /// the arithmetic nests deeper than `max_parse_depth` (the TS overflows its stack)
    Depth,
}

/// `fieldName(instr)` (field-formula.ts:22): the first word in upper case; `=4-1` → `=`.
#[must_use]
pub fn field_name(instr: &[u16]) -> Vec<u16> {
    let t = uo_text::space::trim(instr);
    if t.first() == Some(&u16::from(b'=')) {
        return vec![u16::from(b'=')];
    }
    let first: Vec<u16> = t.iter().copied().take_while(|&u| !is_js_space_unit(u)).collect();
    uo_text::case::to_uppercase(&first).into_owned()
}

/// `hasNestedField(code)` (field-formula.ts:28): `/\{[^{}]*\}/`.
#[must_use]
pub fn has_nested_field(code: Option<&str>) -> bool {
    code.is_some_and(|c| inner_braces(&c.encode_utf16().collect::<Vec<u16>>()).is_some())
}

/// The first `\{([^{}]*)\}` of `s`: (start of `{`, end after `}`).
fn inner_braces(s: &[u16]) -> Option<(usize, usize)> {
    let (open, close) = (u16::from(b'{'), u16::from(b'}'));
    let mut start: Option<usize> = None;
    for (i, &u) in s.iter().enumerate() {
        if u == open {
            start = Some(i);
        } else if u == close
            && let Some(st) = start
        {
            return Some((st, i + 1));
        }
    }
    None
}

/// `evaluateFieldFormula(code, value, lang)` (field-formula.ts:36): `Ok(None)` = cannot compute
/// (nested field without a number, function, bookmark, syntax error, division by zero, or a TS
/// `RangeError` of `toFixed` past 100 digits, accepted by name: the cached result stays).
pub fn evaluate_field_formula(
    code: &[u16],
    value: &mut FieldValue<'_>,
    lang: Option<&str>,
    max_depth: u32,
    cap: &mut TextCap,
) -> Result<Option<Utf16>, FormulaError> {
    let Some(flat) = substitute_nested(code, value, lang, max_depth, cap)? else { return Ok(None) };
    // /^\s*=([^\\]*)((?:\\[\s\S]*)?)$/
    let mut k = 0;
    while flat.get(k).is_some_and(|&u| is_js_space_unit(u)) {
        k += 1;
    }
    if flat.get(k) != Some(&u16::from(b'=')) {
        return Ok(None);
    }
    let body = flat.get(k + 1..).unwrap_or(&[]);
    let bs = body.iter().position(|&u| u == u16::from(b'\\')).unwrap_or(body.len());
    let (expr, switches) = body.split_at(bs);
    let Some((v, decimals)) = parse_arithmetic(expr, max_depth)? else { return Ok(None) };
    if let Some(pic) = find_switch_picture(switches, u16::from(b'#')) {
        return Ok(format_picture(v, &String::from_utf16_lossy(pic)).map(|s| capped(&s, cap)));
    }
    let general = format_switches(switches).into_iter().find(|f| {
        let u = String::from_utf16_lossy(&uo_text::case::to_uppercase(f));
        u != "MERGEFORMAT" && u != "CHARFORMAT"
    });
    if let Some(g) = general {
        let g = String::from_utf16_lossy(g);
        let fmt = general_format(&g).or_else(|| general_format(&uo_text::case::to_uppercase_str(&g)));
        // Word converts only positive integers; the rest stays a number
        if let Some(fmt) = fmt
            && is_integer(v)
            && v > 0.0
        {
            return Ok(Some(format_number(v, Some(fmt), lang.unwrap_or("pt-BR"), cap)));
        }
    }
    Ok(format_formula_result(v, &CellNumber { n: v, decimals, thousands: false, prefix: None, suffix: None }).map(|s| capped(&s, cap)))
}

/// A result text cut at `max_field_text` (K11b, LQ14: the TS has no limit; a `\#` picture is as
/// long as the instruction).
fn capped(s: &str, cap: &mut TextCap) -> Utf16 {
    let mut u: Vec<u16> = s.encode_utf16().collect();
    if u.len() > cap.max {
        u.truncate(cap.max);
        cap.mark_cut();
    }
    Utf16::from_units(u)
}

/// `GENERAL` (field-formula.ts:17): the `\*` formats a formula result takes.
fn general_format(k: &str) -> Option<&'static str> {
    Some(match k {
        "ARABIC" => "decimal",
        "ROMAN" => "upperRoman",
        "roman" => "lowerRoman",
        "ALPHABETIC" => "upperLetter",
        "alphabetic" => "lowerLetter",
        _ => return None,
    })
}

/// `substituteNested` (field-formula.ts:57): each nested `{ … }` (innermost first) replaced by its
/// numeric value, written in the pt-BR notation; at most 200 replacements.
fn substitute_nested(
    code: &[u16],
    value: &mut FieldValue<'_>,
    lang: Option<&str>,
    max_depth: u32,
    cap: &mut TextCap,
) -> Result<Option<Vec<u16>>, FormulaError> {
    let mut out: Vec<u16> = code.to_vec();
    for _ in 0..200 {
        let Some((a, b)) = inner_braces(&out) else { return Ok(Some(out)) };
        let inner: Vec<u16> = out.get(a + 1..b - 1).unwrap_or(&[]).to_vec();
        let name = field_name(&inner);
        let v = if name == [u16::from(b'=')] {
            evaluate_field_formula(&inner, value, lang, max_depth, cap)?.map(|u| u.units().to_vec())
        } else {
            let mut instr = vec![u16::from(b' ')];
            instr.extend_from_slice(uo_text::space::trim(&inner));
            instr.push(u16::from(b' '));
            value(&String::from_utf16_lossy(&name), &instr)
        };
        let Some(v) = v else { return Ok(None) };
        let Some(n) = parse_number(uo_text::space::trim(&v)) else { return Ok(None) };
        let text = to_js_string(n).replacen('.', ",", 1);
        let mut next: Vec<u16> = out.get(..a).unwrap_or(&[]).to_vec();
        next.push(u16::from(b' '));
        next.extend(text.encode_utf16());
        next.push(u16::from(b' '));
        next.extend_from_slice(out.get(b..).unwrap_or(&[]));
        out = next;
    }
    Ok(None)
}

fn is_digit(u: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&u)
}

/// `parseNumber(s)` (field-formula.ts:76): `/^([-+]?)(\d{1,3}(?:\.\d{3})+|\d+)(?:,(\d+))?$/`.
fn parse_number(s: &[u16]) -> Option<f64> {
    let (neg, rest) = match s.first() {
        Some(&u) if u == u16::from(b'-') => (true, s.get(1..).unwrap_or(&[])),
        Some(&u) if u == u16::from(b'+') => (false, s.get(1..).unwrap_or(&[])),
        _ => (false, s),
    };
    let lead = rest.iter().take_while(|&&u| is_digit(u)).count();
    if lead == 0 {
        return None;
    }
    let dot = u16::from(b'.');
    let comma = u16::from(b',');
    // alternative 1: thousands groups
    let mut int: Vec<u16> = Vec::new();
    let mut at = lead;
    let mut groups = 0;
    if lead <= 3 {
        int.extend_from_slice(rest.get(..lead).unwrap_or(&[]));
        while rest.get(at) == Some(&dot) && rest.get(at + 1..at + 4).is_some_and(|g| g.len() == 3 && g.iter().all(|&u| is_digit(u))) {
            int.extend_from_slice(rest.get(at + 1..at + 4).unwrap_or(&[]));
            at += 4;
            groups += 1;
        }
    }
    let tail = |at: usize| -> Option<Option<&[u16]>> {
        match rest.get(at) {
            None => Some(None),
            Some(&u) if u == comma => {
                let d = rest.get(at + 1..).unwrap_or(&[]);
                (!d.is_empty() && d.iter().all(|&u| is_digit(u))).then_some(Some(d))
            }
            _ => None,
        }
    };
    let (int, dec) = if groups > 0
        && let Some(dec) = tail(at)
    {
        (int, dec)
    } else {
        // alternative 2: \d+
        let dec = tail(lead)?;
        (rest.get(..lead).unwrap_or(&[]).to_vec(), dec)
    };
    let mut text = String::from_utf16_lossy(&int);
    if let Some(d) = dec {
        text.push('.');
        text.push_str(&String::from_utf16_lossy(d));
    }
    let n = uo_jsnum::parse::parse_float(&text);
    Some(if neg { -n } else { n })
}

/// One token of `parseArithmetic`: a number literal or one of `-+*/()`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tok<'a> {
    Num(&'a [u16]),
    Op(u16),
}

/// `/\s*(\d[\d.]*(?:,\d+)?|[-+*/()])/y` repeated: the tokens and where they stop.
fn tokenize(src: &[u16]) -> (Vec<Tok<'_>>, usize) {
    let mut toks = Vec::new();
    let mut at = 0usize;
    loop {
        let mut k = at;
        while src.get(k).is_some_and(|&u| is_js_space_unit(u)) {
            k += 1;
        }
        let Some(&u) = src.get(k) else { break };
        if is_digit(u) {
            let mut e = k + 1;
            while src.get(e).is_some_and(|&u| is_digit(u) || u == u16::from(b'.')) {
                e += 1;
            }
            if src.get(e) == Some(&u16::from(b',')) && src.get(e + 1).is_some_and(|&u| is_digit(u)) {
                e += 2;
                while src.get(e).is_some_and(|&u| is_digit(u)) {
                    e += 1;
                }
            }
            toks.push(Tok::Num(src.get(k..e).unwrap_or(&[])));
            at = e;
        } else if b"-+*/()".iter().any(|&c| u16::from(c) == u) {
            toks.push(Tok::Op(u));
            at = k + 1;
        } else {
            break;
        }
    }
    (toks, at)
}

struct Parser<'a> {
    toks: Vec<Tok<'a>>,
    i: usize,
    decimals: f64,
    depth: u32,
    max_depth: u32,
}

/// Why the recursive descent stopped: a syntax error (the TS `SyntaxError`, caught) or the depth.
enum Stop {
    Syntax,
    Depth,
}

impl Parser<'_> {
    fn op(&self) -> Option<u16> {
        match self.toks.get(self.i) {
            Some(Tok::Op(u)) => Some(*u),
            _ => None,
        }
    }

    fn enter(&mut self) -> Result<(), Stop> {
        self.depth += 1;
        if self.depth > self.max_depth { Err(Stop::Depth) } else { Ok(()) }
    }

    fn expr(&mut self) -> Result<f64, Stop> {
        self.enter()?;
        let mut a = self.term()?;
        while let Some(op @ (0x2b | 0x2d)) = self.op() {
            self.i += 1;
            let b = self.term()?;
            a = if op == 0x2b { a + b } else { a - b };
        }
        self.depth -= 1;
        Ok(a)
    }

    fn term(&mut self) -> Result<f64, Stop> {
        let mut a = self.unary()?;
        while let Some(op @ (0x2a | 0x2f)) = self.op() {
            self.i += 1;
            let b = self.unary()?;
            if op == 0x2f && b == 0.0 {
                return Err(Stop::Syntax);
            }
            a = if op == 0x2a { a * b } else { a / b };
        }
        Ok(a)
    }

    fn unary(&mut self) -> Result<f64, Stop> {
        match self.op() {
            Some(0x2d) => {
                self.i += 1;
                self.enter()?;
                let v = -self.unary()?;
                self.depth -= 1;
                Ok(v)
            }
            Some(0x2b) => {
                self.i += 1;
                self.enter()?;
                let v = self.unary()?;
                self.depth -= 1;
                Ok(v)
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<f64, Stop> {
        let t = self.toks.get(self.i).copied();
        self.i += 1;
        match t {
            None => Err(Stop::Syntax),
            Some(Tok::Op(0x28)) => {
                let v = self.expr()?;
                let close = self.toks.get(self.i).copied();
                self.i += 1;
                if close != Some(Tok::Op(0x29)) {
                    return Err(Stop::Syntax);
                }
                Ok(v)
            }
            Some(Tok::Op(_)) => Err(Stop::Syntax),
            Some(Tok::Num(s)) => {
                let n = parse_number(s).ok_or(Stop::Syntax)?;
                // decimals = max(decimals, /,(\d+)$/ length)
                let d = s.iter().rposition(|&u| u == u16::from(b',')).map_or(0, |c| {
                    let tail = s.get(c + 1..).unwrap_or(&[]);
                    if !tail.is_empty() && tail.iter().all(|&u| is_digit(u)) { tail.len() } else { 0 }
                });
                self.decimals = max(self.decimals, f64::from(u32::try_from(d).unwrap_or(u32::MAX)));
                Ok(n)
            }
        }
    }
}

/// `parseArithmetic(src)` (field-formula.ts:84): value and the decimals of the literals.
fn parse_arithmetic(src: &[u16], max_depth: u32) -> Result<Option<(f64, f64)>, FormulaError> {
    let (toks, at) = tokenize(src);
    if !uo_text::space::trim(src.get(at..).unwrap_or(&[])).is_empty() || toks.is_empty() {
        return Ok(None);
    }
    let n = toks.len();
    let mut p = Parser { toks, i: 0, decimals: 0.0, depth: 0, max_depth };
    match p.expr() {
        Ok(v) => {
            if p.i != n || !v.is_finite() {
                return Ok(None);
            }
            Ok(Some((v, p.decimals)))
        }
        Err(Stop::Syntax) => Ok(None),
        Err(Stop::Depth) => Err(FormulaError::Depth),
    }
}

// ─────────────── Word's formula field (WordCraft study, area "fields", item B25) ───────────────

/// The number notation of the formula parser: the UI language's (Word parses with the UI locale: pt-BR `,` decimal,
/// `.` thousands, `;` list separator — measured: `0.256` is 256 and an en-US document gives the same results).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Notation {
    dec: u16,
    thou: u16,
    list: u16,
}

impl Notation {
    fn of(ui_lang: &str) -> Notation {
        match super::numwords::WordsLang::of(ui_lang) {
            // (en: Word's US notation, not measured here)
            super::numwords::WordsLang::En => Notation { dec: u16::from(b'.'), thou: u16::from(b','), list: u16::from(b',') },
            _ => Notation { dec: u16::from(b','), thou: u16::from(b'.'), list: u16::from(b';') },
        }
    }
}

/// Word's formula errors (measured texts in pt-BR; en/es Word's UI strings, unverified).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WordFormulaError {
    DivZero,
    UnexpectedEnd,
    MissingOperator,
    /// the offending token
    Syntax(String),
    /// the bookmark name, upper-cased
    Bookmark(String),
}

impl WordFormulaError {
    /// The text Word writes as the field result, in the UI language.
    #[must_use]
    pub fn text(&self, ui_lang: &str) -> String {
        use super::numwords::WordsLang;
        let l = WordsLang::of(ui_lang);
        match (self, l) {
            (WordFormulaError::DivZero, WordsLang::Pt) => "!Divisão por zero".into(),
            (WordFormulaError::DivZero, WordsLang::Es) => "!División entre cero".into(),
            (WordFormulaError::DivZero, WordsLang::En) => "!Zero Divide".into(),
            (WordFormulaError::UnexpectedEnd, WordsLang::Pt) => "!Fim imprevisto da fórmula".into(),
            (WordFormulaError::UnexpectedEnd, WordsLang::Es) => "!Final inesperado de la fórmula".into(),
            (WordFormulaError::UnexpectedEnd, WordsLang::En) => "!Unexpected End of Formula".into(),
            (WordFormulaError::MissingOperator, WordsLang::Pt) => "!Está faltando operador".into(),
            (WordFormulaError::MissingOperator, WordsLang::Es) => "!Falta un operador".into(),
            (WordFormulaError::MissingOperator, WordsLang::En) => "!Missing Operator".into(),
            (WordFormulaError::Syntax(t), WordsLang::Pt) => format!("!Erro de sintaxe, {t}"),
            (WordFormulaError::Syntax(t), WordsLang::Es) => format!("!Error de sintaxis, {t}"),
            (WordFormulaError::Syntax(t), WordsLang::En) => format!("!Syntax Error, {t}"),
            (WordFormulaError::Bookmark(t), WordsLang::Pt) => format!("!Indicador não definido, {t}"),
            (WordFormulaError::Bookmark(t), WordsLang::Es) => format!("!Marcador no definido, {t}"),
            (WordFormulaError::Bookmark(t), WordsLang::En) => format!("!Undefined Bookmark, {t}"),
        }
    }
}

/// One token of Word's formula.
#[derive(Clone, Debug, PartialEq)]
enum WTok {
    /// a number literal: its value, its decimals, whether it groups thousands, whether it has a decimal part, the text
    Num {
        v: f64,
        decimals: f64,
        thousands: bool,
        frac: bool,
        text: String,
    },
    Ident(String),
    /// an operator or a separator (`+ - * / ^ % ( ) = < > <= >= <>` and the list separator)
    Op(String),
    /// any other character (a syntax error where it is read)
    Stray(String),
}

impl WTok {
    fn text(&self) -> String {
        match self {
            WTok::Num { text, .. } | WTok::Ident(text) | WTok::Op(text) | WTok::Stray(text) => text.clone(),
        }
    }

    fn starts_operand(&self) -> bool {
        matches!(self, WTok::Num { .. } | WTok::Ident(_)) || *self == WTok::Op("(".into())
    }
}

fn wdigit(u: Option<&u16>) -> bool {
    u.is_some_and(|&u| is_digit(u))
}

fn unit_char(u: Option<&u16>) -> char {
    u.and_then(|&u| char::from_u32(u32::from(u))).unwrap_or('0')
}

/// The tokens of Word's formula in a notation: a number is digits, optional thousands groups (exactly three digits
/// after each separator), an optional decimal part; a leading decimal separator followed by digits is a number too
/// (`1,2,3` is two numbers: "Está faltando operador", measured); `1.5` (pt) is `1` and a stray `.` ("Erro de sintaxe,
/// .", measured).
fn wtokenize(src: &[u16], nt: Notation) -> Vec<WTok> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let txt = |a: usize, b: usize| String::from_utf16_lossy(src.get(a..b).unwrap_or(&[]));
    while let Some(&u) = src.get(i) {
        if is_js_space_unit(u) {
            i += 1;
            continue;
        }
        let s = i;
        if is_digit(u) || (u == nt.dec && wdigit(src.get(i + 1))) {
            let mut int = String::new();
            let mut thousands = false;
            while wdigit(src.get(i)) {
                int.push(unit_char(src.get(i)));
                i += 1;
            }
            while !int.is_empty() && src.get(i) == Some(&nt.thou) && (1..=3).all(|k| wdigit(src.get(i + k))) && !wdigit(src.get(i + 4)) {
                for k in 1..=3 {
                    int.push(unit_char(src.get(i + k)));
                }
                i += 4;
                thousands = true;
            }
            let mut frac = String::new();
            let mut has_frac = false;
            if src.get(i) == Some(&nt.dec) && wdigit(src.get(i + 1)) {
                has_frac = true;
                i += 1;
                while wdigit(src.get(i)) {
                    frac.push(unit_char(src.get(i)));
                    i += 1;
                }
            }
            let lit = format!("{}{}{}", if int.is_empty() { "0" } else { &int }, if has_frac { "." } else { "" }, frac);
            let decimals = f64::from(u32::try_from(frac.len()).unwrap_or(0));
            out.push(WTok::Num { v: uo_jsnum::parse::parse_float(&lit), decimals, thousands, frac: has_frac, text: txt(s, i) });
            continue;
        }
        let ch = char::from_u32(u32::from(u)).unwrap_or('\u{fffd}');
        if ch.is_alphabetic() || ch == '_' {
            while src.get(i).and_then(|&u| char::from_u32(u32::from(u))).is_some_and(|c| c.is_alphanumeric() || c == '_') {
                i += 1;
            }
            out.push(WTok::Ident(txt(s, i)));
            continue;
        }
        let two = src.get(i + 1).and_then(|&n| char::from_u32(u32::from(n)));
        if matches!((ch, two), ('<', Some('=' | '>')) | ('>', Some('='))) {
            i += 2;
            out.push(WTok::Op(txt(s, i)));
            continue;
        }
        i += 1;
        // (`:` joins the two cells of a table range, `A1:B2`; where no name is a cell it is an operator nothing reads, a
        // syntax error at it as a stray character)
        if u == nt.list || "+-*/^%()=<>:".contains(ch) {
            out.push(WTok::Op(txt(s, i)));
        } else {
            out.push(WTok::Stray(txt(s, i)));
        }
    }
    out
}

/// The functions of Word's formula field.
const WORD_FUNCTIONS: [&str; 18] = [
    "ABS", "AND", "AVERAGE", "COUNT", "DEFINED", "FALSE", "IF", "INT", "MAX", "MIN", "MOD", "NOT", "OR", "PRODUCT", "ROUND", "SIGN", "SUM",
    "TRUE",
];

/// The list functions: a literal with a decimal part as an argument is a syntax error at that literal (`SUM(1,2)` →
/// "Erro de sintaxe, 1,2", measured; inferred from that one case for the others).
const LIST_FUNCTIONS: [&str; 6] = ["AVERAGE", "COUNT", "MAX", "MIN", "PRODUCT", "SUM"];

/// The value of a name of a table formula: one number (a cell) or a list (a range, `ABOVE`/`LEFT`/…).
#[derive(Clone, Debug, PartialEq)]
pub enum NameValue {
    Num(f64),
    List(Vec<f64>),
}

/// What the names of a formula stand for beyond Word's functions, `TRUE` and `FALSE`: the session's table formula
/// answers its cells, ranges, `ABOVE`/`BELOW`/`LEFT`/`RIGHT` and the bookmarks with a number; the header/footer
/// evaluator knows none ([`NoNames`]: every other name is an undefined bookmark).
pub trait FormulaNames {
    /// Whether `name` is a cell reference (`A1`): `name:name2` is then read as a range.
    fn is_cell(&self, _name: &str) -> bool {
        false
    }
    /// The value of a name that is no function call (`to`: the second cell of a range); `None` = a bookmark.
    fn value(&mut self, _name: &str, _to: Option<&str>) -> Option<NameValue> {
        None
    }
    /// A bookmark's number; `None` = not defined.
    fn bookmark(&mut self, _name: &str) -> Option<f64> {
        None
    }
}

/// No names: the header/footer formula.
pub struct NoNames;

impl FormulaNames for NoNames {}

/// A parsed formula. A run of operators of one precedence (`1+2-3`, `2*3/4`, `2^3^2`) is one [`WNode::Chain`]
/// evaluated left to right, and a run of `%` one [`WNode::Pct`] with its count: the height of the tree grows only
/// through the nesting `WParser::enter` bounds, so a 100,000-term sum overflows neither the evaluation nor the drop.
#[derive(Clone, Debug)]
enum WNode {
    Num(f64),
    Bookmark(String),
    Neg(Box<WNode>),
    /// the operand and how many `%` follow it
    Pct(Box<WNode>, usize),
    /// a comparison (`=`, `<`, `>`, `<=`, `>=`, `<>`)
    Bin(String, Box<WNode>, Box<WNode>),
    /// the first operand, then each operator with its right operand (left-associative)
    Chain(Box<WNode>, Vec<(String, WNode)>),
    Call(String, Vec<WNode>),
    /// `DEFINED(name)`: whether the name is a defined bookmark
    Defined(bool),
    /// a table range or direction (its text, its numbers): a list function takes every number, anything else one
    List(String, Vec<f64>),
}

struct WParser<'n> {
    toks: Vec<WTok>,
    i: usize,
    list: String,
    decimals: f64,
    thousands: bool,
    depth: u32,
    max_depth: u32,
    names: &'n mut dyn FormulaNames,
}

/// Why Word's parse stopped.
enum WStop {
    Err(WordFormulaError),
    Depth,
}

impl From<WordFormulaError> for WStop {
    fn from(e: WordFormulaError) -> WStop {
        WStop::Err(e)
    }
}

type WRes<T> = Result<T, WStop>;

impl WParser<'_> {
    fn peek(&self) -> Option<&WTok> {
        self.toks.get(self.i)
    }

    fn is_op(&self, o: &str) -> bool {
        matches!(self.peek(), Some(WTok::Op(x)) if x == o)
    }

    fn enter(&mut self) -> WRes<()> {
        self.depth += 1;
        if self.depth > self.max_depth { Err(WStop::Depth) } else { Ok(()) }
    }

    /// The error at the current token (the end: "Fim imprevisto").
    fn here(&self) -> WStop {
        match self.peek() {
            None => WStop::Err(WordFormulaError::UnexpectedEnd),
            Some(t) => WStop::Err(WordFormulaError::Syntax(t.text())),
        }
    }

    fn comparison(&mut self) -> WRes<WNode> {
        self.enter()?;
        let mut a = self.additive()?;
        if let Some(WTok::Op(o)) = self.peek()
            && ["=", "<", ">", "<=", ">=", "<>"].contains(&o.as_str())
        {
            let o = o.clone();
            self.i += 1;
            let b = self.additive()?;
            a = WNode::Bin(o, Box::new(a), Box::new(b));
        }
        self.depth -= 1;
        Ok(a)
    }

    fn additive(&mut self) -> WRes<WNode> {
        let a = self.term()?;
        let mut rest = Vec::new();
        while self.is_op("+") || self.is_op("-") {
            let o = self.peek().map(WTok::text).unwrap_or_default();
            self.i += 1;
            rest.push((o, self.term()?));
        }
        Ok(chain(a, rest))
    }

    fn term(&mut self) -> WRes<WNode> {
        let a = self.power()?;
        let mut rest = Vec::new();
        while self.is_op("*") || self.is_op("/") {
            let o = self.peek().map(WTok::text).unwrap_or_default();
            self.i += 1;
            rest.push((o, self.power()?));
        }
        Ok(chain(a, rest))
    }

    fn power(&mut self) -> WRes<WNode> {
        let a = self.unary()?;
        let mut rest = Vec::new();
        while self.is_op("^") {
            self.i += 1;
            rest.push(("^".to_owned(), self.unary()?));
        }
        Ok(chain(a, rest))
    }

    fn unary(&mut self) -> WRes<WNode> {
        self.enter()?;
        let r = if self.is_op("-") {
            self.i += 1;
            self.unary().map(|v| WNode::Neg(Box::new(v)))
        } else if self.is_op("+") {
            self.i += 1;
            self.unary()
        } else {
            self.postfix()
        };
        self.depth -= 1;
        r
    }

    fn postfix(&mut self) -> WRes<WNode> {
        let v = self.primary()?;
        let mut n = 0usize;
        while self.is_op("%") {
            self.i += 1;
            n += 1;
        }
        Ok(if n == 0 { v } else { WNode::Pct(Box::new(v), n) })
    }

    fn close(&mut self) -> WRes<()> {
        if !self.is_op(")") {
            return Err(self.here());
        }
        self.i += 1;
        Ok(())
    }

    /// A name that is no function call (already read): a cell or range, a direction, a bookmark with a number, else an
    /// undefined bookmark (an error when evaluated).
    fn name(&mut self, name: String, up: String) -> WRes<WNode> {
        let mut to = None;
        if self.names.is_cell(&name) && self.is_op(":") {
            self.i += 1;
            match self.peek() {
                Some(WTok::Ident(n)) if self.names.is_cell(n) => {
                    to = Some(n.clone());
                    self.i += 1;
                }
                _ => return Err(self.here()),
            }
        }
        match self.names.value(&name, to.as_deref()) {
            Some(NameValue::Num(v)) => Ok(WNode::Num(v)),
            Some(NameValue::List(v)) => {
                let text = match to {
                    Some(t) => format!("{name}:{t}"),
                    None => name,
                };
                Ok(WNode::List(text, v))
            }
            None => Ok(self.names.bookmark(&name).map_or(WNode::Bookmark(up), WNode::Num)),
        }
    }

    fn primary(&mut self) -> WRes<WNode> {
        let Some(t) = self.peek().cloned() else { return Err(WordFormulaError::UnexpectedEnd.into()) };
        match t {
            WTok::Num { v, decimals, thousands, .. } => {
                self.i += 1;
                self.decimals = max(self.decimals, decimals);
                self.thousands |= thousands;
                Ok(WNode::Num(v))
            }
            WTok::Op(o) if o == "(" => {
                self.i += 1;
                let v = self.comparison()?;
                self.close()?;
                Ok(v)
            }
            WTok::Ident(name) => {
                self.i += 1;
                let up = uo_text::case::to_uppercase_str(&name);
                if !self.is_op("(") {
                    return match up.as_str() {
                        "TRUE" => Ok(WNode::Num(1.0)),
                        "FALSE" => Ok(WNode::Num(0.0)),
                        _ => self.name(name, up),
                    };
                }
                self.i += 1;
                if !WORD_FUNCTIONS.contains(&up.as_str()) {
                    // an unknown function: the error is at the token after its parenthesis (`FOO(1)` → ", 1")
                    return Err(self.here());
                }
                if up == "DEFINED" {
                    // `DEFINED(name)`: whether the bookmark exists (the layout knows no bookmark values: 0)
                    let defined = if let Some(WTok::Ident(n)) = self.peek().cloned() {
                        self.i += 1;
                        self.names.bookmark(&n).is_some()
                    } else {
                        self.comparison()?;
                        false
                    };
                    self.close()?;
                    return Ok(WNode::Defined(defined));
                }
                let mut args = Vec::new();
                if !(matches!(up.as_str(), "TRUE" | "FALSE") && self.is_op(")")) {
                    let list = self.list.clone();
                    loop {
                        if LIST_FUNCTIONS.contains(&up.as_str())
                            && let Some(WTok::Num { frac: true, text, .. }) = self.peek()
                        {
                            return Err(WordFormulaError::Syntax(text.clone()).into());
                        }
                        args.push(self.comparison()?);
                        if self.is_op(&list) {
                            self.i += 1;
                            continue;
                        }
                        break;
                    }
                }
                // a function short of its arguments: the error is where the separator was due (`MOD(-7,3)` in pt-BR is
                // one argument, -7,3: "Erro de sintaxe, )", measured)
                let min = match up.as_str() {
                    "IF" => 3,
                    "ROUND" | "MOD" | "AND" | "OR" => 2,
                    "TRUE" | "FALSE" => 0,
                    _ => 1,
                };
                if args.len() < min {
                    return Err(self.here());
                }
                self.close()?;
                Ok(WNode::Call(up, args))
            }
            WTok::Op(_) | WTok::Stray(_) => Err(self.here()),
        }
    }
}

/// One of Word's formula functions over its evaluated arguments (`xs`: every argument in order). `ROUND` is half away
/// from zero at the decimals (`ROUND(2,345;2)` = 2,35, measured), `INT` truncates, `MOD` takes the dividend's sign.
pub fn word_function(name: &str, xs: &[f64]) -> Result<f64, WordFormulaError> {
    let one = |k: usize| xs.get(k).copied().unwrap_or(0.0);
    let b = |x: bool| if x { 1.0 } else { 0.0 };
    let t = |x: f64| x != 0.0 && !x.is_nan();
    let count = || f64::from(u32::try_from(xs.len()).unwrap_or(u32::MAX));
    Ok(match name {
        "ABS" => abs(one(0)),
        "AND" => b(t(one(0)) && t(one(1))),
        "OR" => b(t(one(0)) || t(one(1))),
        "NOT" => b(!t(one(0))),
        "AVERAGE" => {
            if xs.is_empty() {
                return Err(WordFormulaError::DivZero);
            }
            xs.iter().sum::<f64>() / count()
        }
        "COUNT" => count(),
        "TRUE" => 1.0,
        "FALSE" | "DEFINED" => 0.0,
        "IF" => {
            if t(one(0)) {
                one(1)
            } else {
                one(2)
            }
        }
        "INT" => uo_jsnum::math::trunc(one(0)),
        // (no number — only an empty table list gives none: 0, as the table formula always had)
        "MAX" | "MIN" if xs.is_empty() => 0.0,
        "MAX" => xs.iter().copied().fold(f64::NEG_INFINITY, uo_jsnum::math::max),
        "MIN" => xs.iter().copied().fold(f64::INFINITY, uo_jsnum::math::min),
        "MOD" => {
            if one(1) == 0.0 {
                return Err(WordFormulaError::DivZero);
            }
            uo_jsnum::math::rem(one(0), one(1))
        }
        "PRODUCT" => xs.iter().product(),
        "ROUND" => round_away(one(0), uo_jsnum::math::trunc(one(1))),
        "SIGN" => sign(one(0)),
        "SUM" => xs.iter().sum(),
        _ => return Err(WordFormulaError::Syntax(name.to_owned())),
    })
}

/// `a` and its operators: `a` itself when there is none.
fn chain(a: WNode, rest: Vec<(String, WNode)>) -> WNode {
    if rest.is_empty() { a } else { WNode::Chain(Box::new(a), rest) }
}

/// One binary operator over its evaluated operands.
fn wbin(o: &str, x: f64, y: f64) -> Result<f64, WordFormulaError> {
    let c = |v: bool| if v { 1.0 } else { 0.0 };
    Ok(match o {
        "+" => x + y,
        "-" => x - y,
        "*" => x * y,
        "/" => {
            if y == 0.0 {
                return Err(WordFormulaError::DivZero);
            }
            x / y
        }
        "^" => pow(x, y),
        "=" => c(x == y),
        "<" => c(x < y),
        ">" => c(x > y),
        "<=" => c(x <= y),
        ">=" => c(x >= y),
        _ => c(x != y),
    })
}

fn weval(n: &WNode) -> Result<f64, WordFormulaError> {
    Ok(match n {
        WNode::Num(v) => *v,
        WNode::Bookmark(name) => return Err(WordFormulaError::Bookmark(name.clone())),
        WNode::Neg(a) => -weval(a)?,
        WNode::Pct(a, k) => {
            let mut v = weval(a)?;
            for _ in 0..*k {
                v /= 100.0;
            }
            v
        }
        WNode::Defined(d) => {
            if *d {
                1.0
            } else {
                0.0
            }
        }
        // one number: a list of one, else the syntax error at the list (not measured)
        WNode::List(text, v) => match v.as_slice() {
            [x] => *x,
            _ => return Err(WordFormulaError::Syntax(text.clone())),
        },
        WNode::Bin(o, a, b) => {
            let (x, y) = (weval(a)?, weval(b)?);
            wbin(o, x, y)?
        }
        WNode::Chain(a, rest) => {
            // left to right, as the nested `((a o b) o c)` it stands for: each operand evaluated, then applied
            let mut x = weval(a)?;
            for (o, b) in rest {
                let y = weval(b)?;
                x = wbin(o, x, y)?;
            }
            x
        }
        WNode::Call(f, args) => {
            // a list function takes every number of a table list; the others one number per argument
            let mut xs = Vec::with_capacity(args.len());
            for a in args {
                match a {
                    WNode::List(_, v) if LIST_FUNCTIONS.contains(&f.as_str()) => xs.extend_from_slice(v),
                    _ => xs.push(weval(a)?),
                }
            }
            word_function(f, &xs)?
        }
    })
}

/// What Word's evaluation of an expression gives: the value, the decimals of its literals and whether a literal
/// grouped thousands, or Word's error.
pub type WordValue = Result<(f64, f64, bool), WordFormulaError>;

/// Word's evaluation of a formula expression (no switches); `Err(Depth)` past `max_depth`.
fn word_arithmetic(expr: &[u16], nt: Notation, max_depth: u32, names: &mut dyn FormulaNames) -> Result<WordValue, FormulaError> {
    let toks = wtokenize(expr, nt);
    let list = String::from_utf16_lossy(&[nt.list]);
    let mut p = WParser { toks, i: 0, list, decimals: 0.0, thousands: false, depth: 0, max_depth, names };
    let node = match p.comparison() {
        Ok(n) => n,
        Err(WStop::Depth) => return Err(FormulaError::Depth),
        Err(WStop::Err(e)) => return Ok(Err(e)),
    };
    if let Some(t) = p.peek() {
        return Ok(Err(if t.starts_operand() { WordFormulaError::MissingOperator } else { WordFormulaError::Syntax(t.text()) }));
    }
    // a table list alone is its sum (the table formula's rule)
    let v = match &node {
        WNode::List(_, v) => Ok(v.iter().fold(0.0, |a, x| a + x)),
        n => weval(n),
    };
    Ok(v.map(|v| (v, p.decimals, p.thousands)))
}

/// Word's formula grammar over the names of a table formula (WordCraft study, fields B25: the session's table formula
/// under Word's rules): the expression after `=` (no switches) in the UI language's notation, the names answered by
/// `names`. `Err(Depth)` past `max_depth`.
pub fn evaluate_word_expression(
    expr: &[u16],
    ui_lang: &str,
    max_depth: u32,
    names: &mut dyn FormulaNames,
) -> Result<WordValue, FormulaError> {
    word_arithmetic(expr, Notation::of(ui_lang), max_depth, names)
}

/// `substituteNested` for Word's evaluator: each nested `{ … }` (innermost first) replaced by its numeric value in the
/// notation; a nested formula is Word's; a nested field without a number keeps the cached result (`None`).
fn substitute_nested_word(
    code: &[u16],
    value: &mut FieldValue<'_>,
    words: super::numwords::WordsLang,
    ui_lang: &str,
    max_depth: u32,
    cap: &mut TextCap,
) -> Result<Option<Vec<u16>>, FormulaError> {
    let nt = Notation::of(ui_lang);
    let mut out: Vec<u16> = code.to_vec();
    for _ in 0..200 {
        let Some((a, b)) = inner_braces(&out) else { return Ok(Some(out)) };
        let inner: Vec<u16> = out.get(a + 1..b - 1).unwrap_or(&[]).to_vec();
        let name = field_name(&inner);
        let v = if name == [u16::from(b'=')] {
            evaluate_field_formula_word(&inner, value, words, ui_lang, max_depth, cap)?.map(|u| u.units().to_vec())
        } else {
            let mut instr = vec![u16::from(b' ')];
            instr.extend_from_slice(uo_text::space::trim(&inner));
            instr.push(u16::from(b' '));
            value(&String::from_utf16_lossy(&name), &instr)
        };
        let Some(v) = v else { return Ok(None) };
        let Ok((n, _, _)) = word_arithmetic(uo_text::space::trim(&v), nt, max_depth, &mut NoNames)? else { return Ok(None) };
        let mut text = to_js_string(n);
        if nt.dec != u16::from(b'.') {
            text = text.replacen('.', &String::from_utf16_lossy(&[nt.dec]), 1);
        }
        let mut next: Vec<u16> = out.get(..a).unwrap_or(&[]).to_vec();
        next.push(u16::from(b' '));
        next.extend(text.encode_utf16());
        next.push(u16::from(b' '));
        next.extend_from_slice(out.get(b..).unwrap_or(&[]));
        out = next;
    }
    Ok(None)
}

/// Word's formula field (WordCraft study, area "fields", item B25; measured in Word 365 pt-BR, the 104 `hf-*` rows of
/// `tests/oracle_wc/fields/word-fields-dump.tsv` of uo-docx-layout): comparisons, `^`, postfix `%`, the 18 functions
/// (case-insensitive, `;` between the arguments), Word's error texts with the offending token, the thousands of a
/// grouped literal kept in the result, `\#` pictures, and the `\*` switches of [`super::field_switch`] in the field's
/// language (`words`). Numbers are read in the UI language's notation (`ui_lang`). `Ok(None)`: keep the cached result
/// (a nested field without a number, pt-BR DollarText).
pub fn evaluate_field_formula_word(
    code: &[u16],
    value: &mut FieldValue<'_>,
    words: super::numwords::WordsLang,
    ui_lang: &str,
    max_depth: u32,
    cap: &mut TextCap,
) -> Result<Option<Utf16>, FormulaError> {
    let Some(flat) = substitute_nested_word(code, value, words, ui_lang, max_depth, cap)? else { return Ok(None) };
    let mut k = 0;
    while flat.get(k).is_some_and(|&u| is_js_space_unit(u)) {
        k += 1;
    }
    if flat.get(k) != Some(&u16::from(b'=')) {
        return Ok(None);
    }
    let body = flat.get(k + 1..).unwrap_or(&[]);
    let bs = body.iter().position(|&u| u == u16::from(b'\\')).unwrap_or(body.len());
    let (expr, switches) = body.split_at(bs);
    let (v, decimals, thousands) = match word_arithmetic(expr, Notation::of(ui_lang), max_depth, &mut NoNames)? {
        Ok(r) => r,
        Err(e) => return Ok(Some(capped(&e.text(ui_lang), cap))),
    };
    if let Some(pic) = find_switch_picture(switches, u16::from(b'#')) {
        return Ok(format_picture(v, &String::from_utf16_lossy(pic)).map(|s| capped(&s, cap)));
    }
    let sw = super::field_switch::parse_general(switches);
    if sw.format.is_some() || sw.unknown {
        return Ok(match super::field_switch::apply_number(v, &sw, words, ui_lang) {
            Ok(Some(t)) => Some(capped(&t, cap)),
            Ok(None) => None,
            Err(e) => Some(capped(e, cap)),
        });
    }
    Ok(format_formula_result(v, &CellNumber { n: v, decimals, thousands, prefix: None, suffix: None })
        .map(|s| capped(&super::field_switch::apply_case(&s, &sw.cases), cap)))
}

// ─────────────── docx-core table-formula.ts: the result formats ───────────────

/// `CellNumber` (table-formula.ts:14): a number and the format it passes to the result.
#[derive(Debug, Clone, PartialEq)]
pub struct CellNumber {
    pub n: f64,
    pub decimals: f64,
    pub thousands: bool,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
}

/// `group(digits, sep)`: `digits.replace(/\B(?=(\d{3})+(?!\d))/g, sep)`.
fn group(digits: &str, sep: char) -> String {
    let cs: Vec<char> = digits.chars().collect();
    let word = |i: Option<&char>| i.is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_');
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for i in 0..=cs.len() {
        let before = if i == 0 { false } else { word(cs.get(i - 1)) };
        let after = word(cs.get(i));
        if before == after {
            let k = cs.get(i..).map_or(0, |r| r.iter().take_while(|c| c.is_ascii_digit()).count());
            if k > 0 && k % 3 == 0 {
                out.push(sep);
            }
        }
        if let Some(c) = cs.get(i) {
            out.push(*c);
        }
    }
    out
}

/// `roundAway(v, d)` (table-formula.ts:243): round away from zero.
fn round_away(v: f64, d: f64) -> f64 {
    let p = pow(10.0, d);
    (sign(v) * round(abs(v) * p + 1e-9)) / p
}

/// `x.toFixed(d)` (`None` where V8 throws `RangeError`, d > 100).
fn fixed(x: f64, d: f64) -> Option<String> {
    if !(0.0..=100.0).contains(&d) {
        return None;
    }
    to_fixed(x, uo_jsnum::conv::to_uint32(d)).ok()
}

/// `formatFormulaResult(v, fmt)` (table-formula.ts:249): the decimals needed up to
/// max(2, the sources' decimals), thousands and currency inherited. `None` where the TS throws
/// (`toFixed` past 100 digits).
#[must_use]
pub fn format_formula_result(v: f64, fmt: &CellNumber) -> Option<String> {
    let cap = max(2.0, fmt.decimals);
    let mut d = 0.0;
    while d < cap && abs(round_away(v, d) - v) > 1e-9 {
        d += 1.0;
    }
    if fmt.prefix.is_some() {
        d = max(d, fmt.decimals);
    }
    let r = round_away(v, d);
    let s = fixed(abs(r), d)?;
    let (ip, dp) = match s.split_once('.') {
        Some((a, b)) => (a.to_owned(), Some(b.to_owned())),
        None => (s.clone(), None),
    };
    let mut body = if fmt.thousands { group(&ip, '.') } else { ip };
    if let Some(dp) = dp.filter(|d| !d.is_empty()) {
        body.push(',');
        body.push_str(&dp);
    }
    Some(format!(
        "{}{}{}{}",
        if r < 0.0 { "-" } else { "" },
        fmt.prefix.as_deref().unwrap_or(""),
        body,
        fmt.suffix.as_deref().unwrap_or("")
    ))
}

/// `splitSections(p)` (table-formula.ts:300): `;` outside quotes.
fn split_sections(p: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut q = false;
    for ch in p.chars() {
        if ch == '\'' {
            q = !q;
        }
        if ch == ';' && !q {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(ch);
        }
    }
    out.push(cur);
    out
}

/// `/^'(.*)'$/` → `$1` (`.` does not match line terminators).
fn unquote(p: &str) -> String {
    let lt = |c: char| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}');
    if p.len() >= 2
        && p.starts_with('\'')
        && p.ends_with('\'')
        && let Some(inner) = p.get(1..p.len() - 1)
        && !inner.chars().any(lt)
    {
        return inner.to_owned();
    }
    p.to_owned()
}

fn is_ph(c: char) -> bool {
    matches!(c, '0' | '#' | 'x')
}

/// `formatPicture(v, picture)` (table-formula.ts:263): Word's `\#` picture with the pt-BR
/// separators. `None` where the TS throws (`toFixed` past 100 digits).
#[must_use]
pub fn format_picture(v: f64, picture: &str) -> Option<String> {
    let secs = split_sections(picture);
    let mut pic = secs.first().cloned().unwrap_or_default();
    let mut x = v;
    if v < 0.0 && secs.get(1).is_some() {
        pic = secs.get(1).cloned().unwrap_or_default();
        x = -v;
    } else if v == 0.0 && secs.get(2).is_some() {
        pic = secs.get(2).cloned().unwrap_or_default();
    }
    let pic = unquote(&pic);
    let (int_pic, dec_pic, has_comma) = match pic.find(',') {
        Some(c) => (pic.get(..c).unwrap_or(""), pic.get(c + 1..).unwrap_or(""), true),
        None => (pic.as_str(), "", false),
    };
    let n_dec = dec_pic.chars().filter(|&c| is_ph(c)).count();
    let n_dec_f = f64::from(u32::try_from(n_dec).unwrap_or(u32::MAX));
    let r = round_away(abs(x), n_dec_f);
    let s = fixed(r, n_dec_f)?;
    let (ip, dp) = match s.split_once('.') {
        Some((a, b)) => (a.to_owned(), b.to_owned()),
        None => (s.clone(), String::new()),
    };
    let mut digits: Vec<char> =
        if ip == "0" && int_pic.contains('#') && !int_pic.contains('0') { Vec::new() } else { ip.chars().collect() };
    let chars: Vec<char> = int_pic.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let first_ph = chars.iter().position(|&c| is_ph(c));
    let grouping = {
        let cs: Vec<char> = int_pic.chars().collect();
        cs.windows(3).any(|w| matches!(w, [a, '.', b] if is_ph(*a) && is_ph(*b)))
    };
    for k in (0..chars.len()).rev() {
        let ch = chars.get(k).copied().unwrap_or(' ');
        if is_ph(ch) {
            let d = digits.pop();
            // the remaining digits all go to the first placeholder (grouped by 3 if the picture
            // groups)
            if Some(k) == first_ph && !digits.is_empty() {
                let mut all: String = digits.iter().collect();
                if let Some(d) = d {
                    all.push(d);
                }
                out.insert(0, if grouping { group(&all, '.') } else { all });
                digits.clear();
                continue;
            }
            out.insert(
                0,
                match d {
                    Some(d) => d.to_string(),
                    None if ch == '0' => "0".into(),
                    None if ch == '#' => " ".into(),
                    None => String::new(),
                },
            );
        } else if ch == '.' {
            // thousands separator: only with a digit on its left
            if !digits.is_empty() {
                out.insert(0, ".".into());
            }
        } else {
            out.insert(0, ch.to_string());
        }
    }
    let dpu: Vec<u16> = dp.encode_utf16().collect();
    let mut dec = String::new();
    let mut di = 0usize;
    for ch in dec_pic.chars() {
        if is_ph(ch) {
            match dpu.get(di) {
                Some(&u) => dec.push(char::from_u32(u32::from(u)).unwrap_or('\u{fffd}')),
                None => dec.push(if ch == '0' { '0' } else { ' ' }),
            }
            di += 1;
        } else {
            dec.push(ch);
        }
    }
    let neg = if x < 0.0 && secs.get(1).is_none() { "-" } else { "" };
    let mut res = format!("{neg}{}", out.concat());
    if has_comma {
        res.push(',');
        res.push_str(&dec);
    }
    Some(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn eval(code: &str) -> Option<String> {
        let mut cap = TextCap::default();
        let mut value = |name: &str, _instr: &[u16]| -> Option<Vec<u16>> {
            match name {
                "NUMPAGES" => Some(u("4")),
                "PAGE" => Some(u("1.000")),
                _ => None,
            }
        };
        evaluate_field_formula(&u(code), &mut value, Some("pt-BR"), 256, &mut cap)
            .ok()
            .flatten()
            .map(|v| String::from_utf16_lossy(v.units()))
    }

    #[test]
    fn formulas() {
        assert_eq!(eval("= { NUMPAGES } - 1").as_deref(), Some("3"));
        assert_eq!(eval("=4/3").as_deref(), Some("1,33"));
        assert_eq!(eval("= { PAGE } + 0,5").as_deref(), Some("1000,5"));
        assert_eq!(eval(r#"=2*3 \# "00,0""#).as_deref(), Some("06,0"));
        assert_eq!(eval(r"=14 \* ROMAN").as_deref(), Some("XIV"));
        assert_eq!(eval(r"=14 \* Roman").as_deref(), Some("XIV"));
        assert_eq!(eval("=1/0"), None);
        assert_eq!(eval("= { FOO }"), None);
        assert_eq!(eval("=SUM(1)"), None);
        assert!(has_nested_field(Some("= { PAGE }")));
        assert!(!has_nested_field(Some("= 1")));
    }

    #[test]
    fn deep_nesting_is_a_limit() {
        let deep = format!("={}1{}", "(".repeat(5000), ")".repeat(5000));
        let mut cap = TextCap::default();
        let mut none = |_: &str, _: &[u16]| -> Option<Vec<u16>> { None };
        assert_eq!(evaluate_field_formula(&u(&deep), &mut none, None, 256, &mut cap), Err(FormulaError::Depth));
        let minus = format!("={}1", "-".repeat(100_000));
        assert_eq!(evaluate_field_formula(&u(&minus), &mut none, None, 256, &mut cap), Err(FormulaError::Depth));
        let ok = format!("={}1{}", "(".repeat(100), ")".repeat(100));
        assert_eq!(
            evaluate_field_formula(&u(&ok), &mut none, None, 256, &mut cap).map(|v| v.map(|v| String::from_utf16_lossy(v.units()))),
            Ok(Some("1".into()))
        );
    }

    fn weval_str(code: &str, words: super::super::numwords::WordsLang) -> String {
        let mut cap = TextCap::default();
        let mut value = |_: &str, _: &[u16]| -> Option<Vec<u16>> { None };
        evaluate_field_formula_word(&u(code), &mut value, words, "pt-BR", 256, &mut cap)
            .ok()
            .flatten()
            .map_or_else(|| "<cache>".into(), |v| String::from_utf16_lossy(v.units()))
    }

    /// WordCraft study, item B25: rows of the measured `hf-*` dump (the full 104 rows run in uo-docx-layout's
    /// `word_wc_fields`).
    #[test]
    fn word_formulas() {
        use super::super::numwords::WordsLang;
        let pt = WordsLang::Pt;
        for (code, want) in [
            ("= SUM(1;2;3)", "6"),
            ("= AVERAGE(1;2)", "1,5"),
            ("= IF(1>0;5;6)", "5"),
            ("= DEFINED(nada)", "0"),
            ("= 2^3", "8"),
            ("= 3<>3", "0"),
            ("= 200*10%", "20"),
            ("= INT(-2,5)", "-2"),
            ("= ROUND(2,345;2)", "2,35"),
            ("= ROUND(-2,345;2)", "-2,35"),
            ("= ROUND(1234,5;-2)", "1200"),
            ("= MOD(7;-3)", "1"),
            ("= MOD(7,5;2)", "1,5"),
            ("= 1/0", "!Divisão por zero"),
            ("= SUM(", "!Fim imprevisto da fórmula"),
            ("= 2 3", "!Está faltando operador"),
            ("= FOO(1)", "!Erro de sintaxe, 1"),
            ("= nada", "!Indicador não definido, NADA"),
            ("= (1+2", "!Fim imprevisto da fórmula"),
            ("= 1,2,3", "!Está faltando operador"),
            ("= 1.000,5", "1.000,5"),
            ("= 1.5", "!Erro de sintaxe, ."),
            ("= INT(-2.5)", "!Erro de sintaxe, ."),
            ("= SUM(1,2)", "!Erro de sintaxe, 1,2"),
            ("= 1;2", "!Erro de sintaxe, ;"),
            ("= int(2,5)", "2"),
            ("= Round(2,5;0)", "3"),
            ("= MAX()", "!Erro de sintaxe, )"),
            ("= 1+", "!Fim imprevisto da fórmula"),
            ("= )", "!Erro de sintaxe, )"),
            ("= 2*(3", "!Fim imprevisto da fórmula"),
            ("= 0,5 \\# \"0%\"", "1%"),
            ("= -2,5 \\# \"0,0\"", "-2,5"),
            ("= 5 \\* CardText", "Cinco"),
        ] {
            assert_eq!(weval_str(code, pt), want, "{code}");
        }
        assert_eq!(weval_str("= 5 \\* CardText", WordsLang::En), "five");
        assert_eq!(weval_str("= 14,55 \\* DollarText", WordsLang::En), "fourteen and 55/100");
    }

    #[test]
    fn pictures_and_groups() {
        assert_eq!(group("1234567", '.'), "1.234.567");
        assert_eq!(format_picture(1234.5, "#.##0,00").as_deref(), Some("1.234,50"));
        assert_eq!(format_picture(-5.0, "0;(0)").as_deref(), Some("(5)"));
        assert_eq!(
            format_formula_result(2.0 / 3.0, &CellNumber { n: 0.0, decimals: 0.0, thousands: false, prefix: None, suffix: None })
                .as_deref(),
            Some("0,67")
        );
    }
}
