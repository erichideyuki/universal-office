//! COUNTIF/SUMIF/AVERAGEIFS/D-function criteria and Excel wildcards (`criteria.ts`; rules measured in Excel 365
//! pt-BR, listed in the TS header).
//!
//! The TS returns a closure per criterion; here [`Criterion`] is data (an enum of the TS branches) with
//! [`Criterion::matches`], built once per criterion and matched per cell without allocating for numbers, booleans,
//! errors, ASCII text and wildcards (a non-ASCII text compares through its collation key, prepared once for the
//! criterion side). The wildcard `RegExp` (`^…$`, flag `i` without `u`) is a glob matcher over code units with the
//! `RegExp` `Canonicalize` of each unit: `*` = `[\s\S]*`, `?` = `[\s\S]`, `~` makes the next unit literal (a
//! trailing `~` is literal), which accepts exactly the texts the regular expression accepts.

use std::sync::OnceLock;

use crate::coerce::parse_number_text;
use crate::collate::PreparedText;
use crate::jstext::{upper, with_u16};
use crate::locale::ValueLocale;
use crate::strings::JsStr;
use crate::value::{ErrorCode, ScalarView};

/// `compare15(a, b)`: the criteria's number comparison on 15 significant digits.
#[must_use]
pub fn compare15(a: f64, b: f64) -> i32 {
    if a == b {
        return 0;
    }
    let d = a - b;
    if d.abs() > 1e-13 * uo_jsnum::math::max(a.abs(), b.abs()) {
        return if d < 0.0 { -1 } else { 1 };
    }
    let p15 = |x: f64| uo_jsnum::format::to_precision(x, 15).map_or(x, |s| uo_jsnum::parse::to_number(&s));
    let (x, y) = (p15(a), p15(b));
    if x < y {
        -1
    } else if x > y {
        1
    } else {
        0
    }
}

/// The comparison operator at the start of a text criterion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// No operator.
    None,
    /// `=`.
    Eq,
    /// `<>`.
    Ne,
    /// `<`.
    Lt,
    /// `<=`.
    Le,
    /// `>`.
    Gt,
    /// `>=`.
    Ge,
}

impl Op {
    /// `/^(<=|>=|<>|=|<|>)?/`: the operator and its length in units.
    fn parse(s: &[u16]) -> (Self, usize) {
        let at = |i: usize| s.get(i).copied().unwrap_or(0);
        let (lt, gt, eq_) = (u16::from(b'<'), u16::from(b'>'), u16::from(b'='));
        match (at(0), at(1)) {
            (a, b) if a == lt && b == eq_ => (Self::Le, 2),
            (a, b) if a == gt && b == eq_ => (Self::Ge, 2),
            (a, b) if a == lt && b == gt => (Self::Ne, 2),
            (a, _) if a == eq_ => (Self::Eq, 1),
            (a, _) if a == lt => (Self::Lt, 1),
            (a, _) if a == gt => (Self::Gt, 1),
            _ => (Self::None, 0),
        }
    }

    /// `opPredicate`'s switch on a defined comparison.
    fn holds(self, c: i32) -> bool {
        match self {
            Self::None | Self::Eq => c == 0,
            Self::Lt => c < 0,
            Self::Le => c <= 0,
            Self::Gt => c > 0,
            Self::Ge => c >= 0,
            Self::Ne => c != 0,
        }
    }
}

/// A wildcard token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    /// `*`.
    Any,
    /// `?`.
    One,
    /// A literal unit, canonicalized.
    Lit(u16),
}

/// The `RegExp` `Canonicalize` (flag `i`, no `u`) of every Latin-1 unit.
fn latin1_canon() -> &'static [u16; 256] {
    static T: OnceLock<[u16; 256]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u16; 256];
        for (i, v) in t.iter_mut().enumerate() {
            *v = uo_text::case::regexp_canonicalize(i as u16);
        }
        t
    })
}

#[inline]
fn canon(u: u16) -> u16 {
    if u < 256 { latin1_canon()[usize::from(u)] } else { uo_text::case::regexp_canonicalize(u) }
}

/// A compiled wildcard pattern (`wildcardRegExp(pattern)` with `whole`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wildcard {
    toks: Vec<Tok>,
}

impl Wildcard {
    /// `wildcardRegExp(pattern)`: `~` takes away the meaning of the next unit, whatever it is (`~a*` = `a*`); a
    /// trailing `~` is literal.
    #[must_use]
    pub fn new(pattern: &[u16]) -> Self {
        let mut toks = Vec::with_capacity(pattern.len());
        let mut i = 0;
        while i < pattern.len() {
            let ch = pattern[i];
            if ch == u16::from(b'~') && i + 1 < pattern.len() {
                i += 1;
                toks.push(Tok::Lit(canon(pattern[i])));
            } else if ch == u16::from(b'*') {
                // consecutive stars match the same texts as one
                if toks.last() != Some(&Tok::Any) {
                    toks.push(Tok::Any);
                }
            } else if ch == u16::from(b'?') {
                toks.push(Tok::One);
            } else {
                toks.push(Tok::Lit(canon(ch)));
            }
            i += 1;
        }
        Self { toks }
    }

    /// `re.test(text)` with the pattern anchored at both ends.
    #[must_use]
    pub fn matches(&self, text: JsStr<'_>) -> bool {
        match text {
            JsStr::Latin1(b) => self.glob(b.len(), |i| canon(u16::from(b[i]))),
            JsStr::Utf16(u) => self.glob(u.len(), |i| canon(u[i])),
        }
    }

    /// Linear-backtracking glob (the last `*` is the only resume point it needs).
    fn glob(&self, n: usize, unit: impl Fn(usize) -> u16) -> bool {
        let p = &self.toks;
        let (mut i, mut j) = (0usize, 0usize);
        let mut star: Option<(usize, usize)> = None;
        while i < n {
            match p.get(j) {
                Some(Tok::One) => {
                    i += 1;
                    j += 1;
                }
                Some(&Tok::Lit(c)) if c == unit(i) => {
                    i += 1;
                    j += 1;
                }
                Some(Tok::Any) => {
                    star = Some((j, i));
                    j += 1;
                }
                _ => match star {
                    Some((sj, si)) => {
                        star = Some((sj, si + 1));
                        j = sj + 1;
                        i = si + 1;
                    }
                    None => return false,
                },
            }
        }
        p[j.min(p.len())..].iter().all(|t| *t == Tok::Any)
    }
}

/// `hasWildcard(pattern)`: a `*` or `?` not escaped by `~`.
#[must_use]
pub fn has_wildcard(pattern: &[u16]) -> bool {
    let mut i = 0;
    while i < pattern.len() {
        let ch = pattern[i];
        if ch == u16::from(b'~') {
            i += 2;
            continue;
        }
        if ch == u16::from(b'*') || ch == u16::from(b'?') {
            return true;
        }
        i += 1;
    }
    false
}

/// The pt-BR error names (`i18n/langs/pt-BR.ts` `errors`; the others are the canonical codes). Also VALUETOTEXT/
/// ARRAYTOTEXT's `errorText` (A4d).
pub(crate) fn pt_error_name(e: ErrorCode) -> &'static str {
    match e {
        ErrorCode::Null => "#NULO!",
        ErrorCode::Value => "#VALOR!",
        ErrorCode::Name => "#NOME?",
        ErrorCode::Num => "#NÚM!",
        ErrorCode::NA => "#N/D",
        ErrorCode::GettingData => "#OBTENDO_DADOS",
        ErrorCode::Spill => "#DESPEJAR!",
        ErrorCode::Connect => "#CONECTAR!",
        ErrorCode::Blocked => "#BLOQUEADO!",
        ErrorCode::Field => "#CAMPO!",
        _ => crate::cell::error_text(e),
    }
}

/// `localeErrorCode(text, locale)` = `canonicalError(text, locale.id)` (`i18n/translate.ts:663`): the error named in
/// the calculating language (`#N/D` in pt-BR, where `#N/A` is text), through the A5 language tables
/// ([`crate::i18n::canonical_error`] of [`crate::i18n::resolve_language`]: only `pt-BR`, any case, resolves to the
/// pt-BR table; every other id falls back to English — `pt` alone goes to `pt-PT`, which has no table).
#[must_use]
pub fn locale_error_code(text: &[u16], locale: &ValueLocale) -> Option<ErrorCode> {
    crate::i18n::canonical_error(text, crate::i18n::resolve_language(Some(&locale.id)))
}

/// What a criterion tests (the TS closures, as data).
#[derive(Clone, Debug)]
enum Kind {
    /// `v === null || v === ''`.
    EmptyOrEmptyText,
    /// `v === null`.
    IsEmpty,
    /// `v !== null`.
    NotEmpty,
    /// Nothing matches.
    Never,
    /// A boolean criterion: `v === b`.
    BoolIs(bool),
    /// An error criterion: `v === e`.
    ErrIs(ErrorCode),
    /// `=` on numbers and texts that convert (`numberCriterion` with `''`/`=`).
    NumEq(f64),
    /// `opPredicate` over numbers.
    NumOp(Op, f64),
    /// `opPredicate` over booleans.
    BoolOp(Op, bool),
    /// `opPredicate` over errors (only equality is defined).
    ErrOp(Op, ErrorCode),
    /// Whole-text equality by collation (`<>` = complement).
    TextEq(PreparedText, bool),
    /// Wildcard text (`<>` = complement).
    TextWild(Wildcard, bool),
    /// `<`, `<=`, `>`, `>=` on texts by collation (`*`/`?` literal).
    TextOp(Op, PreparedText),
}

/// A parsed criterion: a predicate over a cell value + `blank` (it matches an empty cell, which whole-column
/// scans must account for).
#[derive(Clone, Debug)]
pub struct Criterion<'l> {
    kind: Kind,
    /// The criterion matches an empty cell.
    pub blank: bool,
    locale: &'l ValueLocale,
    year: f64,
}

/// `parseCriterion(crit, locale, { prefix })`: number, boolean, error or text with an optional `=`, `<>`, `<`,
/// `<=`, `>`, `>=`. `prefix` = database criteria (a text without operator matches the start). `year`: the clock's
/// current year (numbers in the criterion and the cells are read with it, as the TS `parseNumberText`).
#[must_use]
pub fn parse_criterion<'l>(crit: ScalarView<'_>, locale: &'l ValueLocale, year: f64, prefix: bool) -> Criterion<'l> {
    let make = |kind: Kind, blank: bool| Criterion { kind, blank, locale, year };
    let text = match crit {
        ScalarView::Empty => return number_criterion(Op::None, 0.0, locale, year),
        ScalarView::Num(n) => return number_criterion(Op::None, n, locale, year),
        ScalarView::Bool(b) => return make(Kind::BoolIs(b), false),
        ScalarView::Err(e) => return make(Kind::ErrIs(e), false),
        ScalarView::Str(s) => s,
    };
    with_u16(text, |u| {
        let (op, k) = Op::parse(u);
        let rest = &u[k..];
        if rest.is_empty() {
            return match op {
                Op::None => make(Kind::EmptyOrEmptyText, true),
                Op::Eq => make(Kind::IsEmpty, true),
                Op::Ne => make(Kind::NotEmpty, false),
                _ => make(Kind::Never, false),
            };
        }
        // a number in the locale (dates and times included)
        if let Some(n) = parse_number_text(JsStr::Utf16(rest), locale, false, year) {
            return number_criterion(op, n, locale, year);
        }
        // a boolean by the locale's name
        let up = upper(rest);
        let b = if crate::jstext::eq_upper(&up, &locale.true_text) {
            Some(true)
        } else if crate::jstext::eq_upper(&up, &locale.false_text) {
            Some(false)
        } else {
            None
        };
        if let Some(b) = b {
            return make(Kind::BoolOp(op, b), op == Op::Ne);
        }
        // an error by the locale's name
        if let Some(code) = locale_error_code(rest, locale) {
            return make(Kind::ErrOp(op, code), op == Op::Ne);
        }
        // text
        match op {
            Op::None | Op::Eq | Op::Ne => {
                let negate = op == Op::Ne;
                let kind = if op == Op::None && prefix {
                    // database criteria: a text without operator = "starts with" (measured)
                    let mut p = rest.to_vec();
                    p.push(u16::from(b'*'));
                    Kind::TextWild(Wildcard::new(&p), false)
                } else if rest.iter().any(|&c| c == u16::from(b'*') || c == u16::from(b'?')) {
                    Kind::TextWild(Wildcard::new(rest), negate)
                } else {
                    Kind::TextEq(PreparedText::new(JsStr::Utf16(rest)), negate)
                };
                make(kind, negate)
            }
            _ => make(Kind::TextOp(op, PreparedText::new(JsStr::Utf16(rest))), false),
        }
    })
}

/// `numberCriterion`: `=` accepts text that converts; `<>` excludes only numbers; the others only numbers.
fn number_criterion(op: Op, n: f64, locale: &ValueLocale, year: f64) -> Criterion<'_> {
    if op == Op::None || op == Op::Eq {
        return Criterion { kind: Kind::NumEq(n), blank: false, locale, year };
    }
    Criterion { kind: Kind::NumOp(op, n), blank: op == Op::Ne, locale, year }
}

/// `opPredicate(op, cmp)` once `cmp(v)` is known (`None` = incomparable types).
fn op_predicate(op: Op, c: Option<i32>) -> bool {
    match (op, c) {
        (Op::Ne, None) => true,
        (_, None) => false,
        (op, Some(c)) => op.holds(c),
    }
}

impl Criterion<'_> {
    /// The predicate on a cell value.
    #[must_use]
    pub fn matches(&self, v: ScalarView<'_>) -> bool {
        match &self.kind {
            Kind::EmptyOrEmptyText => matches!(v, ScalarView::Empty) || matches!(v, ScalarView::Str(s) if s.is_empty()),
            Kind::IsEmpty => matches!(v, ScalarView::Empty),
            Kind::NotEmpty => !matches!(v, ScalarView::Empty),
            Kind::Never => false,
            Kind::BoolIs(b) => matches!(v, ScalarView::Bool(x) if x == *b),
            Kind::ErrIs(e) => matches!(v, ScalarView::Err(x) if x == *e),
            Kind::NumEq(n) => {
                let x = match v {
                    ScalarView::Num(x) => Some(x),
                    ScalarView::Str(s) => parse_number_text(s, self.locale, false, self.year),
                    _ => None,
                };
                x.is_some_and(|x| compare15(x, *n) == 0)
            }
            Kind::NumOp(op, n) => op_predicate(*op, if let ScalarView::Num(x) = v { Some(compare15(x, *n)) } else { None }),
            Kind::BoolOp(op, b) => op_predicate(
                *op,
                if let ScalarView::Bool(x) = v {
                    Some(if x == *b {
                        0
                    } else if x {
                        1
                    } else {
                        -1
                    })
                } else {
                    None
                },
            ),
            Kind::ErrOp(op, code) => op_predicate(*op, if let ScalarView::Err(x) = v { (x == *code).then_some(0) } else { None }),
            Kind::TextEq(t, negate) => {
                let hit = matches!(v, ScalarView::Str(s) if t.compare_from(s).is_eq());
                hit != *negate
            }
            Kind::TextWild(w, negate) => {
                let hit = matches!(v, ScalarView::Str(s) if w.matches(s));
                hit != *negate
            }
            Kind::TextOp(op, t) => op_predicate(
                *op,
                if let ScalarView::Str(s) = v {
                    Some(match t.compare_from(s) {
                        std::cmp::Ordering::Less => -1,
                        std::cmp::Ordering::Equal => 0,
                        std::cmp::Ordering::Greater => 1,
                    })
                } else {
                    None
                },
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(p: &str, t: &str) -> bool {
        let pu: Vec<u16> = p.encode_utf16().collect();
        let tu: Vec<u16> = t.encode_utf16().collect();
        Wildcard::new(&pu).matches(JsStr::Utf16(&tu))
    }

    #[test]
    fn wildcards() {
        assert!(w("a*", "ABC"));
        assert!(w("*", ""));
        assert!(!w("?", ""));
        assert!(w("~*", "*"));
        assert!(!w("~*", "a"));
        assert!(w("a~", "A~"));
        assert!(w("*b*c", "xxbyyc"));
        assert!(!w("*b*c", "xxbyycd"));
        assert!(w("~a*", "abc"));
    }

    #[test]
    fn criteria() {
        let pt = ValueLocale::pt_br();
        let c = parse_criterion(ScalarView::Str(JsStr::Latin1(b">=2")), &pt, 2025.0, false);
        assert!(c.matches(ScalarView::Num(3.0)));
        assert!(!c.matches(ScalarView::Str(JsStr::Latin1(b"3"))));
        let c = parse_criterion(ScalarView::Str(JsStr::Latin1(b"1")), &pt, 2025.0, false);
        assert!(c.matches(ScalarView::Str(JsStr::Latin1(b" 1"))));
        let nd: Vec<u16> = "#N/D".encode_utf16().collect();
        assert_eq!(locale_error_code(&nd, &pt), Some(ErrorCode::NA));
        assert_eq!(locale_error_code(&nd, &ValueLocale::en_us()), None);
    }
}
