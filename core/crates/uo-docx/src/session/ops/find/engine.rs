//! `find-engine.ts` ported 1:1 (D2b-F9, Opus): Word's Find and Replace — what matches and what a
//! replacement writes (measured in Word: write/tools/home-lab exp-find.ps1). The TS builds a
//! JavaScript `RegExp` source; here the same source is built over UTF-16 units, character for
//! character, and run by the hand matcher of [`super::regex`] (with V8's error messages for the
//! sources V8 refuses, e.g. `e\-mail` from a plain `e-mail`: the TS escapes `-` outside a class,
//! which the `u` flag rejects). Every scan charges the command's [`Budget`].
//!
//! Text: a story is one UTF-16 string, every model unit one unit (objects, notes and deleted text
//! as marks), each paragraph closed by `\r` (`\u001D` at a section break): [`story_text`].

use super::regex::{Budget, BudgetExceeded, Match, Regex, RegexError, Vm};
use crate::model::para::Paragraph;
use crate::model::run::Run;
use crate::pvec::PVec;

pub const PARA: u16 = 0x0D;
pub const SECT: u16 = 0x1D;
pub const FOOTNOTE: u16 = 0x02;
pub const ENDNOTE: u16 = 0x04;
pub const GRAPHIC: u16 = 0x01;
pub const DELETED: u16 = 0x00;
const OBJECT: u16 = 0xFFFC;

/// The format searched for (or applied by a replacement): `FindFormat`. `None` = not given.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FindFormat {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub double_strike: Option<bool>,
    pub all_caps: Option<bool>,
    pub small_caps: Option<bool>,
    pub hidden: Option<bool>,
    /// `'superscript' | 'subscript' | null`
    pub vert_align: Option<Option<String>>,
    /// the Latin font family
    pub font: Option<String>,
    pub size_pt: Option<f64>,
    /// RRGGBB
    pub color: Option<String>,
    /// highlighted (any colour) or not
    pub highlight: Option<bool>,
    /// a character style id
    pub char_style: Option<String>,
    /// a paragraph style id
    pub para_style: Option<String>,
}

/// `FindQuery`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FindQuery {
    pub text: Vec<u16>,
    pub match_case: bool,
    pub whole_word: bool,
    pub wildcards: bool,
    pub sounds_like: bool,
    pub all_forms: bool,
    pub prefix: bool,
    pub suffix: bool,
    pub ignore_punct: bool,
    pub ignore_space: bool,
    pub format: Option<Box<FindFormat>>,
}

/// Why a query does not compile (`FindError` codes, or the `SyntaxError` V8 throws).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// `FindError('pattern', text)`: a wildcard pattern Word refuses
    Pattern,
    /// `FindError('special', '^x')`: the text of the bad special
    Special(Vec<u16>),
    /// `String(e)` of V8's `SyntaxError` for the built source
    Syntax(Vec<u16>),
    /// a construct the builder never emits (never happens; kept so nothing is silently accepted)
    Unsupported,
}

impl QueryError {
    /// `findQueryError`'s message (`findMsg`: Portuguese when the element's language is `pt`).
    #[must_use]
    pub fn message(&self, pt: bool) -> Vec<u16> {
        let s = |t: &str| t.encode_utf16().collect::<Vec<u16>>();
        match self {
            QueryError::Pattern => s(if pt {
                "O texto em \"Localizar\" contém uma expressão não válida para o padrão coincidente."
            } else {
                "The Find What text contains a Pattern Match expression which is not valid."
            }),
            QueryError::Special(sp) => {
                let mut out = s("\"");
                out.extend_from_slice(sp);
                out.extend(s(if pt {
                    "\" não é um caractere especial válido para a caixa Localizar."
                } else {
                    "\" is not a valid special character for the Find What box."
                }));
                out
            }
            QueryError::Syntax(m) => m.clone(),
            QueryError::Unsupported => s("SyntaxError: unsupported"),
        }
    }
}

/// What a search can fail with at run time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindFailure {
    /// the command's step budget ran out (the TS would keep running, or loop forever)
    Budget,
}

impl From<BudgetExceeded> for FindFailure {
    fn from(_: BudgetExceeded) -> FindFailure {
        FindFailure::Budget
    }
}

// ───────────────────────── the story text ─────────────────────────

/// `StoryText`: the text of a sequence of paragraphs of a story.
#[derive(Debug, Clone)]
pub struct StoryText<'a> {
    pub text: Vec<u16>,
    /// the start of each paragraph in the text
    pub starts: Vec<u32>,
    paras: &'a PVec<Paragraph>,
    first: usize,
    count: usize,
    /// `(start, paragraph, run)` of every non-empty run, in order (`runs[g]` of the TS)
    runs: Vec<(u32, u32, u32)>,
}

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `!!str`: a JS string is truthy when not empty.
fn truthy(s: Option<&str>) -> bool {
    s.is_some_and(|s| !s.is_empty())
}

/// `storyText(paras)` over `paras[first .. first + count]`.
pub fn story_text<'a>(paras: &'a PVec<Paragraph>, first: usize, count: usize, budget: &mut Budget) -> Result<StoryText<'a>, FindFailure> {
    let mut text: Vec<u16> = Vec::new();
    let mut starts = Vec::with_capacity(count);
    let mut runs = Vec::new();
    for li in 0..count {
        let Some(p) = paras.get(first + li) else { break };
        starts.push(u32_of(text.len()));
        for (ri, r) in p.runs.iter().enumerate() {
            let n = r.text.len();
            if n == 0 {
                continue;
            }
            budget.charge(n as u64)?;
            runs.push((u32_of(text.len()), u32_of(li), u32_of(ri)));
            let e = r.ext();
            let mark = if r.props.del.is_some() {
                Some(DELETED)
            } else if let Some(nr) = e.and_then(|e| e.note_ref.as_ref()) {
                Some(if &*nr.kind == "endnote" { ENDNOTE } else { FOOTNOTE })
            } else if e.is_some_and(|e| e.image.is_some() || e.shape.is_some() || e.group.is_some() || e.textbox.is_some()) {
                Some(GRAPHIC)
            } else if e.is_some_and(|e| e.custom.is_some()) {
                Some(OBJECT)
            } else {
                None
            };
            match mark {
                Some(m) => text.extend(std::iter::repeat_n(m, n)),
                None => text.extend_from_slice(r.text.units()),
            }
        }
        text.push(if p.props.section_break.is_some() { SECT } else { PARA });
    }
    let count = starts.len();
    Ok(StoryText { text, starts, paras, first, count, runs })
}

impl<'a> StoryText<'a> {
    /// The paragraph `i` of this text.
    #[must_use]
    pub fn para(&self, i: usize) -> Option<&'a Paragraph> {
        if i >= self.count {
            return None;
        }
        self.paras.get(self.first + i)
    }

    /// The number of paragraphs.
    #[must_use]
    pub fn para_count(&self) -> usize {
        self.count
    }

    /// The first paragraph's index in the story.
    #[must_use]
    pub fn first(&self) -> usize {
        self.first
    }

    /// `(paragraph, run)` of the unit `g` (`null` at a paragraph mark: `None`).
    #[must_use]
    pub fn run_index(&self, g: u32) -> Option<(u32, u32)> {
        let i = self.runs.partition_point(|r| r.0 <= g).checked_sub(1)?;
        let &(start, para, run) = self.runs.get(i)?;
        let r = self.para(para as usize)?.runs.get(run as usize)?;
        (g < start + u32_of(r.text.len())).then_some((para, run))
    }

    /// `st.runs[g]`.
    #[must_use]
    pub fn run_at(&self, g: u32) -> Option<&'a Run> {
        let (p, r) = self.run_index(g)?;
        self.para(p as usize)?.runs.get(r as usize)
    }
}

/// `posOf(st, g)`: `(paragraph, offset)` of a text index.
#[must_use]
pub fn pos_of(starts: &[u32], g: u32) -> (u32, u32) {
    let (mut lo, mut hi) = (0usize, starts.len().saturating_sub(1));
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if starts.get(mid).is_some_and(|&s| s <= g) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    (u32_of(lo), g.wrapping_sub(starts.get(lo).copied().unwrap_or(0)))
}

// ───────────────────────── building the source ─────────────────────────

fn push(out: &mut Vec<u16>, s: &str) {
    out.extend(s.encode_utf16());
}

/// `esc(c)`: `\` before each of `\^$.*+?()[]{}|/-`.
fn esc_into(out: &mut Vec<u16>, s: &[u16]) {
    for &u in s {
        if matches!(
            char::from_u32(u32::from(u)),
            Some('\\' | '^' | '$' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '/' | '-')
        ) {
            out.push(u16::from(b'\\'));
        }
        out.push(u);
    }
}

fn esc(s: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len() + 1);
    esc_into(&mut out, s);
    out
}

/// `String.fromCodePoint(n)`.
fn from_code_point(n: u32) -> Vec<u16> {
    let mut out = Vec::with_capacity(2);
    uo_text::utf16::push(&mut out, n);
    out
}

/// `^nnn` with a leading 0 reads in the Windows ANSI page.
fn cp1252(n: u32) -> Option<u32> {
    Some(match n {
        128 => 0x20ac,
        130 => 0x201a,
        131 => 0x192,
        132 => 0x201e,
        133 => 0x2026,
        134 => 0x2020,
        135 => 0x2021,
        136 => 0x2c6,
        137 => 0x2030,
        138 => 0x160,
        139 => 0x2039,
        140 => 0x152,
        142 => 0x17d,
        145 => 0x2018,
        146 => 0x2019,
        147 => 0x201c,
        148 => 0x201d,
        149 => 0x2022,
        150 => 0x2013,
        151 => 0x2014,
        152 => 0x2dc,
        153 => 0x2122,
        154 => 0x161,
        155 => 0x203a,
        156 => 0x153,
        158 => 0x17e,
        159 => 0x178,
        _ => return None,
    })
}

fn is_digit(u: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&u)
}

/// `/^(u?)(\d{1,5})/.exec(code)`: `(has u, value, matched length)`; the value through `parseInt`
/// of at most five digits, then the ANSI page for a leading 0.
fn num_match(code: &[u16]) -> Option<(bool, Vec<u16>)> {
    let u = code.first() == Some(&u16::from(b'u')) && code.get(1).copied().is_some_and(is_digit);
    let digits: Vec<u16> = code.iter().skip(usize::from(u)).take(5).copied().take_while(|&d| is_digit(d)).collect();
    (!digits.is_empty()).then_some((u, digits))
}

fn num_code(code: &[u16]) -> Option<(u32, usize)> {
    let (u, digits) = num_match(code)?;
    let at = usize::from(u);
    let mut n: u32 = digits.iter().fold(0, |v, &d| v * 10 + u32::from(d - u16::from(b'0')));
    if !u
        && digits.first() == Some(&u16::from(b'0'))
        && let Some(m) = cp1252(n)
    {
        n = m;
    }
    Some((n, at + digits.len()))
}

/// `special(code, wild)`: a Word special (`^x`) as a source fragment and the units it took.
fn special(code: &[u16], wild: bool) -> Option<(Vec<u16>, usize)> {
    let c = *code.first()?;
    let mut re = Vec::new();
    if let Some((n, len)) = num_code(code) {
        match n {
            13 => re.extend_from_slice(&[u16::from(b'['), PARA, SECT, u16::from(b']')]),
            12 => push(&mut re, "\\f"),
            11 => push(&mut re, "[\\n\\u2028]"),
            14 => push(&mut re, "\\u000B"),
            30 => push(&mut re, "\\u2011"),
            31 => push(&mut re, "\\u00AD"),
            _ => re = esc(&from_code_point(n)),
        }
        return Some((re, len));
    }
    match char::from_u32(u32::from(c)) {
        Some('p') => re.push(PARA),
        Some('t') => push(&mut re, "\\t"),
        Some('l') => push(&mut re, "[\\n\\u2028]"),
        Some('m') => push(&mut re, "\\f"),
        Some('n') => push(&mut re, "\\u000B"),
        Some('b') => re.push(SECT),
        Some('s') => push(&mut re, "\\u00A0"),
        Some('-') => push(&mut re, "\\u00AD"),
        Some('~') => push(&mut re, "\\u2011"),
        Some('+') => push(&mut re, "\\u2014"),
        Some('=') => push(&mut re, "\\u2013"),
        Some('f') => re.push(FOOTNOTE),
        Some('e') => re.push(ENDNOTE),
        Some('g') => re.push(GRAPHIC),
        Some('^') => push(&mut re, "\\^"),
        Some('w') if !wild => push(&mut re, "[ \\t\\u00A0]+"),
        Some('?') if !wild => {
            push(&mut re, "[^");
            re.extend_from_slice(&[PARA, SECT]);
            push(&mut re, "\\u00AD");
            re.push(DELETED);
            push(&mut re, "]");
        }
        Some('#') if !wild => push(&mut re, "[0-9]"),
        Some('$') if !wild => push(&mut re, "\\p{L}"),
        _ => return None,
    }
    Some((re, 1))
}

const WORD: &str = "[\\p{L}\\p{N}]";

/// A token of the plain text searched for: its fragment and, for a literal, the character.
struct Token {
    re: Vec<u16>,
    lit: Option<Vec<u16>>,
}

/// `plainTokens(text)`.
fn plain_tokens(text: &[u16]) -> Result<Vec<Token>, QueryError> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&ch) = text.get(i) {
        if ch == u16::from(b'^') {
            let rest = text.get(i + 1..).unwrap_or(&[]);
            let Some((re, consumed)) = special(rest, false) else {
                let mut m = vec![u16::from(b'^')];
                m.extend(text.get(i + 1).copied());
                return Err(QueryError::Special(m));
            };
            out.push(Token { re, lit: None });
            i += 1 + consumed;
            continue;
        }
        let (cp, w) = uo_text::utf16::decode_at(text, i).unwrap_or((u32::from(ch), 1));
        let s = from_code_point(cp);
        out.push(Token { re: esc(&s), lit: Some(s) });
        i += w;
    }
    Ok(out)
}

fn starts_with(a: &[u16], s: &str) -> bool {
    let b: Vec<u16> = s.encode_utf16().collect();
    a.starts_with(&b)
}

/// `/^\{(\d+)(?:([,;])(\d*))?\}/.exec(s)`: `(lo, hi, length, separator)` (`hi` and the separator
/// `None` when the optional group did not match).
type Interval = (Vec<u16>, Option<Vec<u16>>, usize, Option<u16>);

fn interval(s: &[u16]) -> Option<Interval> {
    if s.first() != Some(&u16::from(b'{')) {
        return None;
    }
    let lo_len = s.iter().skip(1).take_while(|&&d| is_digit(d)).count();
    if lo_len == 0 {
        return None;
    }
    let lo: Vec<u16> = s.get(1..1 + lo_len)?.to_vec();
    let mut k = 1 + lo_len;
    let mut hi = None;
    let mut sep = None;
    if let Some(&u) = s.get(k).filter(|&&u| u == u16::from(b',') || u == u16::from(b';')) {
        let n = s.iter().skip(k + 1).take_while(|&&d| is_digit(d)).count();
        // the optional group needs the closing brace right after it; otherwise it matches empty
        if s.get(k + 1 + n) == Some(&u16::from(b'}')) {
            hi = Some(s.get(k + 1..k + 1 + n)?.to_vec());
            sep = Some(u);
            k += 1 + n;
        }
    }
    if s.get(k) != Some(&u16::from(b'}')) {
        return None;
    }
    Some((lo, hi, k + 1, sep))
}

/// `parseInt(digits, 10)`.
fn parse_int(d: &[u16]) -> f64 {
    uo_jsnum::parse::parse_int_utf16(d, 10)
}

/// `wildcardRegex(pat)`: Word's wildcards as a source (always case-sensitive).
pub fn wildcard_regex(pat: &[u16]) -> Result<Vec<u16>, QueryError> {
    let mut atoms: Vec<Vec<u16>> = Vec::new();
    let mut depth = 0u32;
    let fail = || QueryError::Pattern;
    let mut i = 0;
    while let Some(&ch) = pat.get(i) {
        match char::from_u32(u32::from(ch)) {
            Some('\\') => {
                let nx = *pat.get(i + 1).ok_or_else(fail)?;
                atoms.push(esc(&[nx]));
                i += 2;
            }
            Some('?') => {
                atoms.push("[^]".encode_utf16().collect());
                i += 1;
            }
            Some('*') => {
                atoms.push("[^]*?".encode_utf16().collect());
                i += 1;
            }
            Some('@') => {
                let last = atoms.pop().ok_or_else(fail)?;
                if starts_with(&last, "(?<") || starts_with(&last, "(?!") || starts_with(&last, "(?=") {
                    return Err(fail());
                }
                let mut a: Vec<u16> = "(?:".encode_utf16().collect();
                a.extend(last);
                push(&mut a, ")+?");
                atoms.push(a);
                i += 1;
            }
            Some('{') => {
                let m = interval(pat.get(i..).unwrap_or(&[]));
                let last = atoms.pop();
                let (Some((lo, hi, len, _)), Some(last)) = (m, last) else { return Err(fail()) };
                let hi_s: Vec<u16> = hi.clone().unwrap_or_else(|| lo.clone());
                if !hi_s.is_empty() && parse_int(&hi_s) < parse_int(&lo) {
                    return Err(fail());
                }
                let mut a: Vec<u16> = "(?:".encode_utf16().collect();
                a.extend(last);
                push(&mut a, "){");
                a.extend_from_slice(&lo);
                if hi.is_some() {
                    a.push(u16::from(b','));
                    a.extend_from_slice(&hi_s);
                }
                a.push(u16::from(b'}'));
                atoms.push(a);
                i += len;
            }
            Some('<') => {
                atoms.push(format!("(?<!{WORD})(?={WORD})").encode_utf16().collect());
                i += 1;
            }
            Some('>') => {
                atoms.push(format!("(?<={WORD})(?!{WORD})").encode_utf16().collect());
                i += 1;
            }
            Some('[') => {
                let close = u16::from(b']');
                let end = pat.iter().enumerate().skip(i + 2).find(|(_, u)| **u == close).map(|(k, _)| k).ok_or_else(fail)?;
                let mut body: &[u16] = pat.get(i + 1..end).unwrap_or(&[]);
                let mut neg = false;
                if body.first() == Some(&u16::from(b'!')) {
                    neg = true;
                    body = body.get(1..).unwrap_or(&[]);
                }
                if body.is_empty() {
                    return Err(fail());
                }
                let mut cls: Vec<u16> = Vec::new();
                let mut k = 0;
                while let Some(&c0) = body.get(k) {
                    let mut c = c0;
                    if c == u16::from(b'\\') && k + 1 < body.len() {
                        k += 1;
                        c = body.get(k).copied().unwrap_or(c);
                    }
                    if body.get(k + 1) == Some(&u16::from(b'-')) && k + 2 < body.len() {
                        let to = body.get(k + 2).copied().unwrap_or(0);
                        if to < c {
                            return Err(fail());
                        }
                        esc_into(&mut cls, &[c]);
                        cls.push(u16::from(b'-'));
                        esc_into(&mut cls, &[to]);
                        k += 2;
                    } else {
                        esc_into(&mut cls, &[c]);
                    }
                    k += 1;
                }
                let mut a = vec![u16::from(b'[')];
                if neg {
                    a.push(u16::from(b'^'));
                }
                a.extend(cls);
                a.push(close);
                atoms.push(a);
                i = end + 1;
            }
            Some('(') => {
                depth += 1;
                atoms.push(vec![u16::from(b'(')]);
                i += 1;
            }
            Some(')') => {
                if depth == 0 {
                    return Err(fail());
                }
                depth -= 1;
                // the whole group becomes one atom (for @ and {n}); `k = -1` when its `(` was
                // taken by a quantifier: the TS then wraps everything and replaces only the
                // last atom (`splice(-1, …)`)
                let open = [u16::from(b'(')];
                let k = atoms.iter().rposition(|a| a.as_slice() == open);
                let from = k.map_or(0, |k| k + 1);
                let mut grp = vec![u16::from(b'(')];
                for a in atoms.get(from..).unwrap_or(&[]) {
                    grp.extend_from_slice(a);
                }
                grp.push(u16::from(b')'));
                match k {
                    Some(k) => atoms.truncate(k),
                    None => {
                        atoms.pop();
                    }
                }
                atoms.push(grp);
                i += 1;
            }
            Some('^') => {
                let (re, consumed) = special(pat.get(i + 1..).unwrap_or(&[]), true).ok_or_else(fail)?;
                atoms.push(re);
                i += 1 + consumed;
            }
            _ => {
                let (cp, w) = uo_text::utf16::decode_at(pat, i).unwrap_or((u32::from(ch), 1));
                atoms.push(esc(&from_code_point(cp)));
                i += w;
            }
        }
    }
    if depth != 0 {
        return Err(fail());
    }
    Ok(atoms.concat())
}

/// A compiled query: the matcher, and its source and flags (for the messages and the capture).
#[derive(Debug, Clone)]
pub struct Compiled {
    pub re: Regex,
    pub source: Vec<u16>,
    pub flags: &'static str,
}

fn syntax_error(source: &[u16], flags: &str, e: RegexError) -> QueryError {
    match e {
        RegexError::Syntax(why) => {
            let mut m: Vec<u16> = "SyntaxError: Invalid regular expression: /".encode_utf16().collect();
            m.extend_from_slice(source);
            push(&mut m, &format!("/{flags}: {why}"));
            QueryError::Syntax(m)
        }
        RegexError::Unsupported => QueryError::Unsupported,
    }
}

fn letters_or_quote(text: &[u16]) -> bool {
    !text.is_empty() && uo_text::utf16::code_points(text).all(|cp| cp == 0x27 || uo_text::props::is_letter(cp))
}

fn is_caret_d(text: &[u16]) -> bool {
    text == [u16::from(b'^'), u16::from(b'd')]
}

/// `compileQuery(q)`: `None` for a format-only query, `^d`, or sounds-like/all-forms over a word.
pub fn compile_query(q: &FindQuery) -> Result<Option<Compiled>, QueryError> {
    if q.text.is_empty() || (!q.wildcards && is_caret_d(&q.text)) {
        return Ok(None);
    }
    if q.wildcards {
        let source = wildcard_regex(&q.text)?;
        let re = Regex::new(&source, false).map_err(|e| syntax_error(&source, "gu", e))?;
        return Ok(Some(Compiled { re, source, flags: "gu" }));
    }
    if (q.sounds_like || q.all_forms) && letters_or_quote(&q.text) {
        return Ok(None);
    }
    let toks = plain_tokens(&q.text)?;
    let fill: &str = match (q.ignore_punct, q.ignore_space) {
        (true, true) => "[\\p{P} \\t]*",
        (true, false) => "\\p{P}*",
        (false, true) => "[ \\t]*",
        (false, false) => "",
    };
    let space = [u16::from(b' ')];
    let mut body: Vec<u16> = Vec::new();
    for (k, t) in toks.iter().enumerate() {
        let is_punct = t.lit.as_deref().is_some_and(|l| uo_text::utf16::code_points(l).any(uo_text::props::is_punctuation));
        let is_space = t.lit.as_deref() == Some(&space[..]);
        let prev_space = k > 0 && toks.get(k - 1).and_then(|p| p.lit.as_deref()) == Some(&space[..]);
        if k > 0 && !fill.is_empty() && !is_punct && !is_space && !prev_space {
            push(&mut body, fill);
        }
        if is_space && q.ignore_space {
            push(&mut body, " +");
        } else {
            body.extend_from_slice(&t.re);
        }
    }
    let has_space = q.text.iter().any(|&u| uo_text::space::is_space(u));
    let wrap = |pre: &str, b: Vec<u16>, post: &str| {
        let mut o: Vec<u16> = pre.encode_utf16().collect();
        o.extend(b);
        push(&mut o, post);
        o
    };
    if q.whole_word && !has_space {
        body = wrap(&format!("(?<!{WORD})(?:"), body, &format!(")(?!{WORD})"));
    } else {
        if q.prefix {
            body = wrap(&format!("(?<!{WORD})(?:"), body, ")");
        }
        if q.suffix {
            body = wrap("(?:", body, ")(?![\\p{L}\\p{N}'’])");
        }
    }
    let flags = if q.match_case { "gu" } else { "giu" };
    let re = Regex::new(&body, !q.match_case).map_err(|e| syntax_error(&body, flags, e))?;
    Ok(Some(Compiled { re, source: body, flags }))
}

// ───────────────────────── formatting ─────────────────────────

fn lower(s: &str) -> String {
    uo_text::case::to_lowercase_str(s)
}

fn upper(s: &str) -> String {
    uo_text::case::to_uppercase_str(s)
}

/// `runMatches(r, f)`: the run has the format searched for (the paragraph style is the paragraph's).
#[must_use]
pub fn run_matches(r: &Run, f: &FindFormat) -> bool {
    let p = &*r.props;
    let flag = |want: Option<bool>, have: Option<bool>| want.is_none_or(|w| have.unwrap_or(false) == w);
    if !flag(f.bold, p.bold)
        || !flag(f.italic, p.italic)
        || !flag(f.underline, p.underline)
        || !flag(f.strike, p.strike)
        || !flag(f.double_strike, p.double_strike)
        || !flag(f.all_caps, p.all_caps)
        || !flag(f.small_caps, p.small_caps)
        || !flag(f.hidden, p.hidden)
    {
        return false;
    }
    if let Some(v) = &f.vert_align
        && p.vert_align.as_deref() != v.as_deref()
    {
        return false;
    }
    if let Some(font) = &f.font
        && lower(p.font_ascii.as_deref().unwrap_or("")) != lower(font)
    {
        return false;
    }
    if let Some(sz) = f.size_pt {
        let have = match p.sz_half_pt {
            Some(crate::js::JsNumber::Num(n)) => n,
            _ => 22.0,
        };
        #[allow(clippy::float_cmp)]
        if have != uo_jsnum::math::round(sz * 2.0) {
            return false;
        }
    }
    if let Some(color) = &f.color
        && upper(p.color.as_deref().unwrap_or("000000")) != upper(color)
    {
        return false;
    }
    if let Some(h) = f.highlight
        && (truthy(p.highlight.as_deref()) && p.highlight.as_deref() != Some("none")) != h
    {
        return false;
    }
    if let Some(cs) = &f.char_style
        && p.char_style.as_deref().unwrap_or("") != cs.as_str()
    {
        return false;
    }
    true
}

/// `hasRunFormat(f)`: a format key other than the paragraph style.
#[must_use]
pub fn has_run_format(f: Option<&FindFormat>) -> bool {
    f.is_some_and(|f| {
        f.bold.is_some()
            || f.italic.is_some()
            || f.underline.is_some()
            || f.strike.is_some()
            || f.double_strike.is_some()
            || f.all_caps.is_some()
            || f.small_caps.is_some()
            || f.hidden.is_some()
            || f.vert_align.is_some()
            || f.font.is_some()
            || f.size_pt.is_some()
            || f.color.is_some()
            || f.highlight.is_some()
            || f.char_style.is_some()
    })
}

// ───────────────────────── sounds like, word forms ─────────────────────────

/// `metaphone(word)` (Lawrence Philips, 1990), over the units of the word.
#[must_use]
pub fn metaphone(word: &[u16]) -> String {
    let up = uo_text::case::to_uppercase(word);
    let mut w: Vec<u8> = up.iter().filter_map(|&u| u8::try_from(u).ok().filter(u8::is_ascii_uppercase)).collect();
    if w.is_empty() {
        return String::new();
    }
    if w.starts_with(b"KN") || w.starts_with(b"GN") || w.starts_with(b"PN") || w.starts_with(b"AE") || w.starts_with(b"WR") {
        w.remove(0);
    }
    if let Some(f) = w.first_mut().filter(|f| **f == b'X') {
        *f = b'S';
    }
    if w.starts_with(b"WH") {
        w.remove(1);
    }
    let vowel = |c: Option<u8>| c.is_some_and(|c| b"AEIOU".contains(&c));
    let mut out = String::new();
    let n = w.len();
    let mut i = 0;
    while i < n {
        let at = |k: usize| w.get(k).copied();
        let ch = at(i).unwrap_or(0);
        let prev = i.checked_sub(1).and_then(at);
        let next = at(i + 1);
        let next2 = at(i + 2);
        if Some(ch) == prev && ch != b'C' {
            i += 1;
            continue;
        }
        match ch {
            b'A' | b'E' | b'I' | b'O' | b'U' => {
                if i == 0 {
                    out.push(char::from(ch));
                }
            }
            b'B' => {
                if !(prev == Some(b'M') && i == n - 1) {
                    out.push('B');
                }
            }
            b'C' => {
                if next == Some(b'I') && next2 == Some(b'A') {
                    out.push('X');
                } else if next == Some(b'H') {
                    out.push(if prev == Some(b'S') { 'K' } else { 'X' });
                    i += 1;
                } else if matches!(next, Some(b'I' | b'E' | b'Y')) {
                    if prev != Some(b'S') {
                        out.push('S');
                    }
                } else {
                    out.push('K');
                }
            }
            b'D' => {
                if next == Some(b'G') && matches!(next2, Some(b'E' | b'Y' | b'I')) {
                    out.push('J');
                    i += 1;
                } else {
                    out.push('T');
                }
            }
            b'G' => 'g: {
                if next == Some(b'H') && !(i + 2 >= n || vowel(next2)) {
                    break 'g;
                }
                if next == Some(b'N') && (i + 2 == n || (at(i + 2) == Some(b'E') && at(i + 3) == Some(b'D') && i + 4 == n)) {
                    break 'g;
                }
                if prev == Some(b'G') {
                    break 'g;
                }
                out.push(if matches!(next, Some(b'I' | b'E' | b'Y')) { 'J' } else { 'K' });
                if next == Some(b'H') {
                    i += 1;
                }
            }
            b'H' => {
                // `!'CSPTG'.includes(prev ?? '')`: '' is in every string, so never at the start
                if vowel(next) && prev.is_some_and(|p| !b"CSPTG".contains(&p)) {
                    out.push('H');
                }
            }
            b'K' => {
                if prev != Some(b'C') {
                    out.push('K');
                }
            }
            b'P' => {
                if next == Some(b'H') {
                    out.push('F');
                    i += 1;
                } else {
                    out.push('P');
                }
            }
            b'Q' => out.push('K'),
            b'S' => {
                if next == Some(b'H') {
                    out.push('X');
                    i += 1;
                } else if next == Some(b'I') && matches!(next2, Some(b'O' | b'A')) {
                    out.push('X');
                } else {
                    out.push('S');
                }
            }
            b'T' => {
                if next == Some(b'I') && matches!(next2, Some(b'O' | b'A')) {
                    out.push('X');
                } else if next == Some(b'H') {
                    out.push('0');
                    i += 1;
                } else if !(next == Some(b'C') && next2 == Some(b'H')) {
                    out.push('T');
                }
            }
            b'V' => out.push('F'),
            b'W' | b'Y' => {
                if vowel(next) {
                    out.push(char::from(ch));
                }
            }
            b'X' => out.push_str("KS"),
            b'Z' => out.push('S'),
            _ => out.push(char::from(ch)),
        }
        i += 1;
    }
    out
}

/// The irregular English forms: each line a family.
const IRREGULAR: &[&str] = &[
    "be am is are was were been being",
    "have has had having",
    "do does did done doing",
    "go goes went gone going",
    "sit sits sat sitting",
    "run runs ran running",
    "eat eats ate eaten eating",
    "see sees saw seen seeing",
    "come comes came coming",
    "take takes took taken taking",
    "make makes made making",
    "get gets got gotten getting",
    "give gives gave given giving",
    "know knows knew known knowing",
    "think thinks thought thinking",
    "write writes wrote written writing",
    "speak speaks spoke spoken speaking",
    "buy buys bought buying",
    "bring brings brought bringing",
    "begin begins began begun beginning",
    "swim swims swam swum swimming",
    "drink drinks drank drunk drinking",
    "sing sings sang sung singing",
    "fly flies flew flown flying",
    "drive drives drove driven driving",
    "ride rides rode ridden riding",
    "find finds found finding",
    "tell tells told telling",
    "say says said saying",
    "pay pays paid paying",
    "leave leaves left leaving",
    "feel feels felt feeling",
    "keep keeps kept keeping",
    "meet meets met meeting",
    "stand stands stood standing",
    "understand understands understood understanding",
    "teach teaches taught teaching",
    "catch catches caught catching",
    "fall falls fell fallen falling",
    "forget forgets forgot forgotten forgetting",
    "choose chooses chose chosen choosing",
    "break breaks broke broken breaking",
    "wear wears wore worn wearing",
    "grow grows grew grown growing",
    "draw draws drew drawn drawing",
    "show shows showed shown showing",
    "lie lies lay lain lying",
    "die dies died dying",
    "mouse mice",
    "man men",
    "woman women",
    "child children",
    "foot feet",
    "tooth teeth",
    "goose geese",
    "person people",
    "good better best",
    "bad worse worst",
    "far farther further farthest furthest",
    "little less least",
    "many more most",
];

fn is_vowel_unit(u: Option<u16>) -> bool {
    u.is_some_and(|u| matches!(char::from_u32(u32::from(u)), Some('a' | 'e' | 'i' | 'o' | 'u')))
}

fn ends_with(w: &[u16], s: &str) -> bool {
    let b: Vec<u16> = s.encode_utf16().collect();
    w.ends_with(&b)
}

fn cat(a: &[u16], s: &str) -> Vec<u16> {
    let mut o = a.to_vec();
    push(&mut o, s);
    o
}

fn drop_last(w: &[u16], n: usize) -> &[u16] {
    w.get(..w.len().saturating_sub(n)).unwrap_or(&[])
}

/// `regularForms(root)`.
fn regular_forms(root: &[u16]) -> Vec<Vec<u16>> {
    let mut out = vec![root.to_vec()];
    let last = root.last().copied();
    let prev = root.len().checked_sub(2).and_then(|k| root.get(k)).copied();
    let is = |u: Option<u16>, ch: char| u == Some(ch as u16);
    if ["s", "x", "z", "ch", "sh"].iter().any(|s| ends_with(root, s)) {
        out.push(cat(root, "es"));
    } else if is(last, 'y') && !is_vowel_unit(prev) {
        out.push(cat(drop_last(root, 1), "ies"));
    } else {
        out.push(cat(root, "s"));
    }
    // /[^aeiou][aeiou][bcdfgklmnprstvz]$/ over units, for short roots
    let cvc = root.len() <= 4 && root.len() >= 3 && {
        let n = root.len();
        let (a, b, c) = (root.get(n - 3).copied(), root.get(n - 2).copied(), root.get(n - 1).copied());
        !is_vowel_unit(a) && is_vowel_unit(b) && c.is_some_and(|c| "bcdfgklmnprstvz".encode_utf16().any(|x| x == c))
    };
    let stem: Vec<u16> = if cvc { root.iter().copied().chain(last).collect() } else { root.to_vec() };
    if is(last, 'e') {
        out.push(cat(root, "d"));
        out.push(cat(drop_last(root, 1), "ing"));
        out.push(cat(root, "r"));
        out.push(cat(root, "st"));
    } else if is(last, 'y') && !is_vowel_unit(prev) {
        out.push(cat(drop_last(root, 1), "ied"));
        out.push(cat(root, "ing"));
        out.push(cat(drop_last(root, 1), "ier"));
        out.push(cat(drop_last(root, 1), "iest"));
    } else {
        for s in ["ed", "ing", "er", "est"] {
            out.push(cat(&stem, s));
        }
    }
    out
}

/// `rootsOf(w)`: the roots a form may come from (insertion order of the TS `Set`).
fn roots_of(w: &[u16]) -> Vec<Vec<u16>> {
    let mut out: Vec<Vec<u16>> = vec![w.to_vec()];
    let mut add = |s: Vec<u16>| {
        if s.len() >= 2 && !out.contains(&s) {
            out.push(s);
        }
    };
    if ends_with(w, "ies") {
        add(cat(drop_last(w, 3), "y"));
    }
    if ends_with(w, "es") {
        add(drop_last(w, 2).to_vec());
    }
    if ends_with(w, "s") {
        add(drop_last(w, 1).to_vec());
    }
    for suf in ["ed", "ing", "er", "est"] {
        if !ends_with(w, suf) {
            continue;
        }
        let b = drop_last(w, suf.len());
        add(b.to_vec());
        add(cat(b, "e"));
        if ends_with(b, "i") {
            add(cat(drop_last(b, 1), "y"));
        }
        let n = b.len();
        if n >= 3 && b.get(n - 1) == b.get(n - 2) {
            add(drop_last(b, 1).to_vec());
        }
    }
    out
}

/// `wordForms(word)`: every English form of the word (lowercase).
#[must_use]
pub fn word_forms(word: &[u16]) -> Vec<Vec<u16>> {
    let w = uo_text::case::to_lowercase(word).into_owned();
    let mut out: Vec<Vec<u16>> = vec![w.clone()];
    let add = |out: &mut Vec<Vec<u16>>, s: Vec<u16>| {
        if !out.contains(&s) {
            out.push(s);
        }
    };
    for fam in IRREGULAR {
        let forms: Vec<Vec<u16>> = fam.split(' ').map(|f| f.encode_utf16().collect()).collect();
        if forms.contains(&w) {
            for f in forms {
                add(&mut out, f);
            }
        }
    }
    if out.len() > 1 {
        return out;
    }
    for r in roots_of(&w) {
        for f in regular_forms(&r) {
            add(&mut out, f);
        }
    }
    out
}

// ───────────────────────── the search ─────────────────────────

/// `FindHit`: `[start, end)` in the text, and the wildcard groups (`\1…\9`) when the pattern has
/// any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindHit {
    pub start: u32,
    pub end: u32,
    pub groups: Option<Vec<Vec<u16>>>,
}

/// The next word of the sounds-like scan (`/[\p{L}'’]+/gu` from `from`): its start, the end of the
/// match, and the end without the trailing quotes (`m[0].replace(/['’]+$/, '')`).
fn next_word(text: &[u16], from: usize) -> Option<(usize, usize, usize)> {
    let word_char = |cp: u32| cp == 0x27 || cp == 0x2019 || uo_text::props::is_letter(cp);
    let mut g = from;
    let start = loop {
        let (cp, w) = uo_text::utf16::decode_at(text, g)?;
        if word_char(cp) {
            break g;
        }
        g += w;
    };
    while let Some((cp, w)) = uo_text::utf16::decode_at(text, g) {
        if !word_char(cp) {
            break;
        }
        g += w;
    }
    let mut wend = g;
    while wend > start && text.get(wend - 1).is_some_and(|&u| u == 0x27 || u == 0x2019) {
        wend -= 1;
    }
    Some((start, g, wend))
}

/// `FindEnv`: the effective language of a run (all forms: English only) and the effective
/// paragraph style.
pub trait FindEnv {
    fn lang(&self, run: &Run, para: &Paragraph) -> Option<String>;
    fn para_style(&self, para: &Paragraph) -> Option<String>;
}

/// No environment (`{}`): every run English, the paragraph style `props.styleId`.
pub struct NoEnv;

impl FindEnv for NoEnv {
    fn lang(&self, _: &Run, _: &Paragraph) -> Option<String> {
        None
    }
    fn para_style(&self, p: &Paragraph) -> Option<String> {
        p.props.style_id.as_deref().map(str::to_owned)
    }
}

/// `/^en\b/i.test(lang)`.
fn is_english(lang: &str) -> bool {
    let b = lang.as_bytes();
    b.len() >= 2
        && b.first().is_some_and(|c| c.eq_ignore_ascii_case(&b'e'))
        && b.get(1).is_some_and(|c| c.eq_ignore_ascii_case(&b'n'))
        && !b.get(2).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

struct Checks<'q, 's, 'e> {
    st: &'q StoryText<'s>,
    f: Option<&'q FindFormat>,
    fmt_runs: bool,
    env: &'e dyn FindEnv,
}

impl Checks<'_, '_, '_> {
    fn para_ok(&self, g: u32) -> bool {
        let Some(want) = self.f.and_then(|f| f.para_style.as_deref()) else { return true };
        let Some(p) = self.st.para(pos_of(&self.st.starts, g).0 as usize) else { return false };
        let have = self.env.para_style(p).or_else(|| p.props.style_id.as_deref().map(str::to_owned));
        have.as_deref() == Some(want)
    }

    fn char_ok(&self, g: u32) -> bool {
        if !self.para_ok(g) {
            return false;
        }
        if !self.fmt_runs {
            return true;
        }
        // the paragraph mark takes the paragraph's (Word includes it in a formatted span)
        match (self.st.run_at(g), self.f) {
            (Some(r), Some(f)) => run_matches(r, f),
            _ => true,
        }
    }
}

/// `findAll(st, q, env)`: every occurrence in order (the compiled query passed in, so a command
/// compiles once).
pub fn find_all(
    st: &StoryText<'_>,
    q: &FindQuery,
    compiled: Option<&Compiled>,
    env: &dyn FindEnv,
    budget: &mut Budget,
    vm: &mut Vm,
) -> Result<Vec<FindHit>, FindFailure> {
    let f = q.format.as_deref();
    let fmt_runs = has_run_format(f);
    let ck = Checks { st, f, fmt_runs, env };
    let text = &st.text;
    let len = u32_of(text.len());
    let mut out = Vec::new();
    if q.text.is_empty() {
        let Some(f) = f else { return Ok(out) };
        if !fmt_runs && f.para_style.is_none() {
            return Ok(out);
        }
        let mut s: Option<u32> = None;
        for g in 0..=len {
            budget.charge(1)?;
            let ok = g < len
                && text.get(g as usize) != Some(&DELETED)
                && if fmt_runs { st.run_at(g).is_some() && ck.char_ok(g) } else { ck.para_ok(g) };
            if ok && s.is_none() {
                s = Some(g);
            }
            if !ok && let Some(a) = s.take() {
                out.push(FindHit { start: a, end: g, groups: None });
            }
        }
        return Ok(out);
    }
    if !q.wildcards && is_caret_d(&q.text) {
        let mut g0: Option<u32> = None;
        for g in 0..=len {
            budget.charge(1)?;
            let ri = if g < len { st.run_index(g) } else { None };
            let is_field = ri.and_then(|_| st.run_at(g)).is_some_and(|r| {
                r.ext().is_some_and(|e| {
                    truthy(e.field.as_deref()) || e.field_result.is_some() || truthy(e.field_begin.as_deref()) || e.form_field.is_some()
                })
            });
            let prev = g.checked_sub(1).and_then(|p| st.run_index(p));
            if is_field && (g0.is_none() || prev != ri) {
                if let Some(a) = g0 {
                    out.push(FindHit { start: a, end: g, groups: None });
                }
                g0 = Some(g);
            } else if !is_field && let Some(a) = g0.take() {
                out.push(FindHit { start: a, end: g, groups: None });
            }
        }
        let mut kept = Vec::with_capacity(out.len());
        for h in out {
            budget.charge(u64::from(h.end - h.start))?;
            if (h.start..h.end).all(|g| ck.char_ok(g)) {
                kept.push(h);
            }
        }
        return Ok(kept);
    }
    let Some(c) = compiled else {
        // sounds like / all forms: word by word
        let key = if q.sounds_like { metaphone(&q.text) } else { String::new() };
        let forms = q.all_forms.then(|| word_forms(&q.text));
        let q_lower = uo_text::case::to_lowercase(&q.text).into_owned();
        let mut g = 0usize;
        while let Some((start, end, wend)) = next_word(text, g) {
            budget.charge((end - g) as u64 + 1)?;
            g = end;
            let word = text.get(start..wend).unwrap_or(&[]);
            if word.is_empty() {
                continue;
            }
            let ok = match &forms {
                Some(forms) => {
                    let r = st.run_at(u32_of(start));
                    let p = st.para(pos_of(&st.starts, u32_of(start)).0 as usize);
                    let en = match (r, p) {
                        (Some(r), Some(p)) => env.lang(r, p).is_none_or(|l| is_english(&l)),
                        _ => true,
                    };
                    let wl = uo_text::case::to_lowercase(word).into_owned();
                    if en { forms.contains(&wl) } else { wl == q_lower }
                }
                None => metaphone(word) == key,
            };
            if !ok {
                continue;
            }
            budget.charge((wend - start) as u64)?;
            if (u32_of(start)..u32_of(wend)).all(|x| ck.char_ok(x)) {
                out.push(FindHit { start: u32_of(start), end: u32_of(wend), groups: None });
            }
        }
        return Ok(out);
    };
    let limit = c.re.scan_limit(text, budget)?;
    let mut last = 0usize;
    while let Some(Match { start, end, groups }) = c.re.exec(text, last, limit, budget, vm)? {
        last = end as usize;
        if start == end {
            last += 1;
            continue;
        }
        budget.charge(u64::from(end - start))?;
        if (start..end).any(|g| text.get(g as usize) == Some(&DELETED) || !ck.char_ok(g)) {
            continue;
        }
        let groups = (c.re.captures() > 0).then(|| {
            groups.iter().map(|g| g.and_then(|(a, b)| text.get(a as usize..b as usize)).map(<[u16]>::to_vec).unwrap_or_default()).collect()
        });
        out.push(FindHit { start, end, groups });
    }
    Ok(out)
}

// ───────────────────────── replacing ─────────────────────────

fn letters_of(s: &[u16]) -> Vec<u16> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some((cp, w)) = uo_text::utf16::decode_at(s, i) {
        if uo_text::props::is_letter(cp) {
            out.extend_from_slice(s.get(i..i + w).unwrap_or(&[]));
        }
        i += w;
    }
    out
}

/// `adaptCase(found, repl)`: the replacement takes the case of what was found (without "Match case").
#[must_use]
pub fn adapt_case(found: &[u16], repl: &[u16]) -> Vec<u16> {
    let letters = letters_of(found);
    if letters.is_empty() {
        return repl.to_vec();
    }
    let up = uo_text::case::to_uppercase(&letters);
    let lo = uo_text::case::to_lowercase(&letters);
    if *up == letters[..] && *lo != letters[..] {
        return uo_text::case::to_uppercase(repl).into_owned();
    }
    let first = letters.get(..1).unwrap_or(&[]);
    if *uo_text::case::to_uppercase(first) == *first && *uo_text::case::to_lowercase(first) != *first {
        let mut i = 0;
        while let Some((cp, w)) = uo_text::utf16::decode_at(repl, i) {
            if uo_text::props::is_letter(cp) {
                let mut out = repl.get(..i).unwrap_or(&[]).to_vec();
                out.extend_from_slice(&uo_text::case::to_uppercase(repl.get(i..=i).unwrap_or(&[])));
                out.extend_from_slice(repl.get(i + 1..).unwrap_or(&[]));
                return out;
            }
            i += w;
        }
        return repl.to_vec();
    }
    repl.to_vec()
}

/// `expandReplacement(repl, found, groups, wildcards, clipboard)`: the replacement with its
/// specials (`\r` = a new paragraph; `\f`, `\n`, `\t`… as in the model).
#[must_use]
pub fn expand_replacement(repl: &[u16], found: &[u16], groups: Option<&[Vec<u16>]>, wildcards: bool, clipboard: &[u16]) -> Vec<u16> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&ch) = repl.get(i) {
        if ch == u16::from(b'\\') && wildcards {
            match repl.get(i + 1).copied() {
                Some(d) if (u16::from(b'1')..=u16::from(b'9')).contains(&d) => {
                    let k = usize::from(d - u16::from(b'1'));
                    if let Some(g) = groups.and_then(|g| g.get(k)) {
                        out.extend_from_slice(g);
                    }
                    i += 2;
                    continue;
                }
                Some(d) => {
                    out.push(d);
                    i += 2;
                    continue;
                }
                None => {}
            }
        }
        if ch == u16::from(b'^') {
            let c = repl.get(i + 1).copied();
            let mapped: Option<u16> = c.and_then(|c| match char::from_u32(u32::from(c))? {
                'p' => Some(0x0D),
                't' => Some(0x09),
                'l' => Some(0x0A),
                'm' => Some(0x0C),
                'n' => Some(0x0B),
                's' => Some(0xA0),
                '-' => Some(0xAD),
                '~' => Some(0x2011),
                '+' => Some(0x2014),
                '=' => Some(0x2013),
                '^' => Some(u16::from(b'^')),
                _ => None,
            });
            if c == Some(u16::from(b'&')) {
                out.extend_from_slice(found);
                i += 2;
                continue;
            }
            if c == Some(u16::from(b'c')) {
                out.extend_from_slice(clipboard);
                i += 2;
                continue;
            }
            if let Some(m) = mapped {
                out.push(m);
                i += 2;
                continue;
            }
            if let Some((n, len)) = num_code(repl.get(i + 1..).unwrap_or(&[])) {
                match n {
                    13 => out.push(0x0D),
                    11 => out.push(0x0A),
                    12 => out.push(0x0C),
                    9 => out.push(0x09),
                    _ => uo_text::utf16::push(&mut out, n),
                }
                i += 1 + len;
                continue;
            }
        }
        out.push(ch);
        i += 1;
    }
    out
}

/// `foundText(st, h)`: the text found (marks back to readable characters: ¶ = `\r`).
#[must_use]
pub fn found_text(st: &StoryText<'_>, h: &FindHit) -> Vec<u16> {
    st.text.get(h.start as usize..h.end as usize).unwrap_or(&[]).iter().map(|&u| if u == SECT { PARA } else { u }).collect()
}

/// The hand matchers of the regex sites of `find-engine.ts` (`js/regex-sites/d2b-f9.mjs`; the rows
/// return ASCII: UTF-16 units in hex).
#[doc(hidden)]
#[must_use]
pub fn site(id: &str, s: &[u16]) -> Option<String> {
    let hx = |t: &[u16]| t.iter().map(|c| format!("{c:x}")).collect::<Vec<_>>().join(".");
    let ascii = |t: &[u16]| String::from_utf16_lossy(t);
    Some(match id {
        "d2b-f9.esc" => hx(&esc(s)),
        "d2b-f9.num" => match num_match(s) {
            Some((u, d)) => format!("{}|{}", if u { "u" } else { "" }, ascii(&d)),
            None => "null".to_owned(),
        },
        "d2b-f9.interval" => match interval(s) {
            Some((lo, hi, len, sep)) => {
                format!("{len}|{}|{}|{}", ascii(&lo), sep.map_or("~".to_owned(), |c| ascii(&[c])), hi.map_or("~".to_owned(), |h| ascii(&h)))
            }
            None => "null".to_owned(),
        },
        "d2b-f9.lookaround_atom" => (starts_with(s, "(?<") || starts_with(s, "(?!") || starts_with(s, "(?=")).to_string(),
        "d2b-f9.word_or_quote" => letters_or_quote(s).to_string(),
        "d2b-f9.punct" => uo_text::utf16::decode_at(s, 0).is_some_and(|(cp, _)| uo_text::props::is_punctuation(cp)).to_string(),
        "d2b-f9.space" => s.iter().any(|&u| uo_text::space::is_space(u)).to_string(),
        "d2b-f9.words" => {
            let mut out = Vec::new();
            let mut g = 0;
            while let Some((start, end, wend)) = next_word(s, g) {
                out.push(format!("{start}:{}", hx(s.get(start..wend).unwrap_or(&[]))));
                g = end;
            }
            if out.is_empty() { "-".to_owned() } else { out.join(" ") }
        }
        "d2b-f9.en" => is_english(&ascii(s)).to_string(),
        "d2b-f9.letters" => {
            let first = (0..s.len()).find(|&i| {
                uo_text::utf16::decode_at(s, i).is_some_and(|(cp, _)| uo_text::props::is_letter(cp))
                    && !(i > 0
                        && s.get(i).is_some_and(|&u| (0xDC00..0xE000).contains(&u))
                        && s.get(i - 1).is_some_and(|&u| (0xD800..0xDC00).contains(&u)))
            });
            format!("{}|{}", hx(&letters_of(s)), first.map_or(-1, |i| i64::try_from(i).unwrap_or(-1)))
        }
        "d2b-f9.digit19" => s.first().is_some_and(|&d| (u16::from(b'1')..=u16::from(b'9')).contains(&d)).to_string(),
        "d2b-f9.found_text" => hx(&s.iter().map(|&u| if u == SECT { PARA } else { u }).collect::<Vec<_>>()),
        _ => return None,
    })
}
