//! The formula translator canonical ↔ Excel language (`i18n/translate.ts`): text → text, no tree. A port of the
//! TS tokenizer, the operator rewrite of the file form (`_xlfn.SINGLE(x)` → `@x`, `ANCHORARRAY(A1)` → `A1#`,
//! `_TRO_*` → `:.`/`.:`/`.:.`), the LET/LAMBDA parameter scope and the renderer, with the same rules.
//!
//! **Shapes (PERF)**: tokens are spans into the input (no string per token); the [`Translator`] keeps every buffer
//! (input widened to UTF-16, tokens, structured-reference items, folded words, parameter names) between calls, so
//! a translation allocates only its output once the buffers have grown. Function, error, keyword and boolean
//! lookups fold the word into a reused UTF-8 buffer and binary-search the generated tables
//! ([`super::tables`]). Nothing recurses: the operator rewrite runs on an explicit stack.
//!
//! **Declared differences** (hostile input only): an EMPTY list/array separator never matches (the TS loops
//! forever on `startsWith('')`); the case-insensitive R1C1 letters compare by simple case folding through the
//! root lower-case mapping (+ `ſ` → `s`), which is the `iu` `RegExp` rule for every letter the tables hold (R, C,
//! L).

use super::{EN_US, FormulaLanguage, Kw, get_language};
use crate::catalogue::{entry_exact, operator_of, strip_upper_prefix, upper_utf8_into};
use crate::strings::{JsStr, JsString};
use crate::value::ErrorCode;

/// Which language table the options name (`lang?: string | FormulaLanguage`).
#[derive(Clone, Copy, Debug, Default)]
pub enum LangSpec<'a> {
    /// Absent: en-US.
    #[default]
    Default,
    /// An id (`"pt-BR"`); an id without a table resolves to English with the host's `Intl` decimal
    /// ([`SyntaxOptions::regional_decimal`]).
    Id(&'a str),
    /// A table.
    Table(&'static FormulaLanguage),
}

/// The options of the translators and of [`resolve_syntax`] (`SyntaxOptions` + `TranslateOptions`).
#[derive(Clone, Copy, Debug, Default)]
pub struct SyntaxOptions<'a> {
    /// The Excel language.
    pub lang: LangSpec<'a>,
    /// Decimal separator (regional).
    pub decimal: Option<JsStr<'a>>,
    /// List separator (regional).
    pub list: Option<JsStr<'a>>,
    /// Array-constant column separator.
    pub array_col: Option<JsStr<'a>>,
    /// Array-constant row separator.
    pub array_row: Option<JsStr<'a>>,
    /// R1C1 notation.
    pub r1c1: bool,
    /// `fromLocal` only: write the OOXML prefixes.
    pub file_prefixes: bool,
    /// The table the formula is in (`[@Col]` on screen).
    pub in_table: Option<JsStr<'a>>,
    /// The workbook's defined names (they win over a function used as a value).
    pub names: Option<&'a NameSet>,
    /// Host hook C1: `Intl.NumberFormat(id).formatToParts(1.5)` decimal for an id without a table
    /// (`regionalDecimal`; `None` = `Intl` gave none).
    pub regional_decimal: Option<JsStr<'a>>,
}

/// The separators in force for some options (`ResolvedSyntax`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSyntax {
    /// The language table.
    pub lang: &'static FormulaLanguage,
    /// Decimal separator.
    pub decimal: JsString,
    /// List separator.
    pub list: JsString,
    /// Array column separator.
    pub array_col: JsString,
    /// Array row separator.
    pub array_row: JsString,
    /// R1C1.
    pub r1c1: bool,
}

/// `resolveSyntax(opts)`.
#[must_use]
pub fn resolve_syntax(opts: &SyntaxOptions<'_>) -> ResolvedSyntax {
    let mut d = Dialect::default();
    d.resolve(opts);
    ResolvedSyntax {
        lang: d.lang,
        decimal: JsString::from_utf16(&d.decimal),
        list: JsString::from_utf16(&d.list),
        array_col: JsString::from_utf16(&d.acol),
        array_row: JsString::from_utf16(&d.arow),
        r1c1: d.r1c1,
    }
}

/// The defined names of a workbook, folded once (`names` of `TranslateOptions`; the fold is the root upper case of
/// both tables).
#[derive(Clone, Debug, Default)]
pub struct NameSet {
    folded: Vec<String>,
}

impl NameSet {
    /// Folds and sorts the names.
    #[must_use]
    pub fn new<'a>(names: impl IntoIterator<Item = JsStr<'a>>) -> Self {
        let mut folded: Vec<String> = names
            .into_iter()
            .map(|n| {
                let mut s = String::new();
                upper_utf8_into(n, &mut s);
                s
            })
            .collect();
        folded.sort_unstable();
        folded.dedup();
        Self { folded }
    }
    fn contains(&self, f: &str) -> bool {
        self.folded.binary_search_by(|x| x.as_str().cmp(f)).is_ok()
    }
    fn is_empty(&self) -> bool {
        self.folded.is_empty()
    }
}

// ------------------------------------------------------------------ dialect

#[derive(Debug)]
struct Dialect {
    canonical: bool,
    lang: &'static FormulaLanguage,
    decimal: Vec<u16>,
    list: Vec<u16>,
    acol: Vec<u16>,
    arow: Vec<u16>,
    r1c1: bool,
    open: Vec<u16>,
    close: Vec<u16>,
    row: Vec<u16>,
    col: Vec<u16>,
    /// `fold(bools[0])`, `fold(bools[1])`.
    bool_f: [String; 2],
}

impl Default for Dialect {
    fn default() -> Self {
        Self {
            canonical: false,
            lang: EN_US,
            decimal: Vec::new(),
            list: Vec::new(),
            acol: Vec::new(),
            arow: Vec::new(),
            r1c1: false,
            open: Vec::new(),
            close: Vec::new(),
            row: Vec::new(),
            col: Vec::new(),
            bool_f: [String::new(), String::new()],
        }
    }
}

fn set_units(v: &mut Vec<u16>, s: JsStr<'_>) {
    v.clear();
    v.extend(s.units());
}

fn set_str(v: &mut Vec<u16>, s: &str) {
    v.clear();
    v.extend(s.encode_utf16());
}

impl Dialect {
    fn finish(&mut self) {
        let l = self.lang;
        set_str(&mut self.open, l.r1c1.and_then(|r| r.open).unwrap_or("["));
        set_str(&mut self.close, l.r1c1.and_then(|r| r.close).unwrap_or("]"));
        set_str(&mut self.row, l.row_letter());
        set_str(&mut self.col, l.col_letter());
        let b = l.bool_texts();
        for k in 0..2 {
            if b[k].is_ascii() {
                upper_utf8_into(JsStr::Latin1(b[k].as_bytes()), &mut self.bool_f[k]);
            } else {
                let units: Vec<u16> = b[k].encode_utf16().collect();
                upper_utf8_into(JsStr::Utf16(&units), &mut self.bool_f[k]);
            }
        }
    }

    /// The canonical dialect (`CANONICAL` with the `r1c1` flag).
    fn canonical(&mut self, r1c1: bool) {
        self.canonical = true;
        self.lang = EN_US;
        set_str(&mut self.decimal, ".");
        set_str(&mut self.list, ",");
        set_str(&mut self.acol, ",");
        set_str(&mut self.arow, ";");
        self.r1c1 = r1c1;
        self.finish();
    }

    /// `resolveSyntax(opts)` (not canonical).
    fn resolve(&mut self, o: &SyntaxOptions<'_>) {
        self.canonical = false;
        let (lang, intl) = match o.lang {
            LangSpec::Default => (EN_US, None),
            LangSpec::Table(t) => (t, None),
            LangSpec::Id(id) => match get_language(id) {
                Some(t) => (t, None),
                None => (EN_US, o.regional_decimal),
            },
        };
        self.lang = lang;
        match (o.decimal, intl) {
            (Some(d), _) | (None, Some(d)) => set_units(&mut self.decimal, d),
            (None, None) => set_str(&mut self.decimal, lang.regional.map_or(".", |r| r.decimal)),
        }
        match o.list {
            Some(l) => set_units(&mut self.list, l),
            None => {
                let reg = if o.decimal.is_none() && intl.is_none() { lang.regional.map(|r| r.list) } else { None };
                match reg {
                    Some(l) => set_str(&mut self.list, l),
                    None => set_str(&mut self.list, if self.decimal == [u16::from(b',')] { ";" } else { "," }),
                }
            }
        }
        match o.array_col {
            Some(c) => set_units(&mut self.acol, c),
            None => {
                set_str(&mut self.acol, lang.array.map_or(",", |a| a.col));
                if self.acol == self.decimal {
                    set_str(&mut self.acol, lang.array.and_then(|a| a.alt).unwrap_or("\\"));
                }
            }
        }
        match o.array_row {
            Some(r) => set_units(&mut self.arow, r),
            None => set_str(&mut self.arow, lang.array.map_or(";", |a| a.row)),
        }
        self.r1c1 = o.r1c1;
        self.finish();
    }
}

// ------------------------------------------------------------------ tokens

type Sp = (u32, u32);

#[derive(Clone, Copy, Debug)]
enum Txt {
    Span(Sp),
    Lit(&'static str),
}

/// A function name of a token: a table name, or (canonical input with a doubled prefix, `_xlfn._xlfn.SUM`) the
/// upper-cased stripped word in the dynamic buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Canon {
    Static(&'static str),
    Dyn(u32, u32),
}

#[derive(Clone, Copy, Debug)]
enum Part {
    Absent,
    Bare,
    Rel(Sp),
    Abs(Sp),
}

#[derive(Clone, Copy, Debug)]
enum NameS {
    /// Text span; `unesc` = a quoted name (`''` → `'`).
    Span(Sp, bool),
    /// `$<col letters>$<row digits>` of an absolute R1C1 typed in A1 mode.
    AbsRc(u32, Sp),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pre {
    None,
    Pm,
    Op,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SepK {
    List,
    Col,
    Row,
}

#[derive(Clone, Copy, Debug)]
enum SItem {
    Kw(Kw),
    Col(Sp),
    Range(Sp, Sp),
    Raw(Sp),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    Empty,
    Simple,
    Groups,
}

#[derive(Clone, Copy, Debug)]
struct SBody {
    form: Form,
    this_row: bool,
    /// Items in [`Translator::items`].
    items: (u32, u32),
}

#[derive(Clone, Copy, Debug)]
enum Tok {
    Text(Txt),
    Ws(Sp),
    Num { int: Sp, frac: Option<Sp>, exp: Sp },
    Fn { canon: Option<Canon>, raw: Sp },
    Eta { canon: Canon, raw: Sp },
    Name { s: NameS, pre: Pre, quoted: bool },
    Bool(bool),
    Err(ErrorCode),
    Sep(SepK),
    R1c1 { row: Part, col: Part },
    Sref { table: Sp, body: SBody },
    Open,
    Close,
}

fn is_digit(u: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&u)
}

fn mask(u: u16) -> u8 {
    if (0xD800..0xE000).contains(&u) { 0 } else { uo_text::props::mask(u32::from(u)) }
}

/// `/[\p{L}_\\$]/u` on one unit.
fn id_start(u: u16) -> bool {
    mask(u) & uo_text::props::LETTER != 0 || u == u16::from(b'_') || u == u16::from(b'\\') || u == u16::from(b'$')
}

/// `/[\p{L}\p{M}\p{N}_.\\?$]/u` on one unit.
fn id_char(u: u16) -> bool {
    mask(u) & (uo_text::props::LETTER | uo_text::props::MARK | uo_text::props::NUMBER) != 0 || matches!(u, 0x5F | 0x2E | 0x5C | 0x3F | 0x24)
}

fn is_ws(u: u16) -> bool {
    uo_jsnum::text::is_js_space_unit(u)
}

fn starts(t: &[u16], i: usize, p: &[u16]) -> bool {
    !p.is_empty() && t.len() >= i + p.len() && t[i..i + p.len()] == *p
}

/// ASCII case-insensitive prefix (`toLowerCase().startsWith(p)` for an ASCII `p`: a non-ASCII unit never lowers to
/// ASCII here except U+212A KELVIN → `k` and U+0130 → `i̇`, neither of which starts these prefixes).
fn starts_ascii_ci(t: &[u16], p: &str) -> bool {
    t.len() >= p.len() && t.iter().zip(p.bytes()).all(|(&u, b)| u < 0x80 && (u as u8).eq_ignore_ascii_case(&b))
}

/// Simple case folding of one unit (the `iu` `RegExp` comparison).
fn simple_fold(u: u16) -> u32 {
    let cp = u32::from(u);
    if u == 0x17F {
        return u32::from(b's');
    }
    match uo_text::case::lower_mapping(cp) {
        uo_text::case::CaseMapping::One(m) => m,
        _ => cp,
    }
}

fn ci_match(t: &[u16], i: usize, p: &[u16]) -> bool {
    !p.is_empty() && t.len() >= i + p.len() && t[i..i + p.len()].iter().zip(p).all(|(&a, &b)| a == b || simple_fold(a) == simple_fold(b))
}

/// `String(Number(digits))`-free range checks: the value of an ASCII digit run, saturating at `u64::MAX`.
fn digits_value(d: &[u16]) -> u64 {
    d.iter().fold(0u64, |acc, &u| acc.saturating_mul(10).saturating_add(u64::from(u - u16::from(b'0'))))
}

/// `isA1Cell(s)`: `^\$?([A-Za-z]{1,3})\$?([0-9]+)$` within the grid.
fn is_a1_cell(s: &[u16]) -> bool {
    let mut i = 0;
    if s.first() == Some(&u16::from(b'$')) {
        i = 1;
    }
    let l0 = i;
    while i < s.len() && s[i] < 0x80 && (s[i] as u8).is_ascii_alphabetic() {
        i += 1;
    }
    let letters = &s[l0..i];
    if letters.is_empty() || letters.len() > 3 {
        return false;
    }
    if s.get(i) == Some(&u16::from(b'$')) {
        i += 1;
    }
    let d = &s[i..];
    if d.is_empty() || !d.iter().all(|&u| is_digit(u)) {
        return false;
    }
    let c = letters.iter().fold(0u32, |c, &u| c * 26 + u32::from((u as u8).to_ascii_uppercase() - b'A' + 1));
    let r = digits_value(d);
    c <= 16_384 && (1..=1_048_576).contains(&r)
}

fn col_letters(mut n: u32, out: &mut Vec<u16>) {
    let start = out.len();
    while n > 0 {
        out.insert(start, u16::from(b'A') + ((n - 1) % 26) as u16);
        n = (n - 1) / 26;
    }
}

/// `matchBracket(t, i)`: the `]` closing the `[` at `i` (`'` escapes the next unit); `None` = unbalanced.
fn match_bracket(t: &[u16], i: usize) -> Option<usize> {
    let mut depth = 0i64;
    let mut j = i;
    while j < t.len() {
        let c = t[j];
        if c == u16::from(b'\'') {
            j += 2;
            continue;
        }
        if c == u16::from(b'[') {
            depth += 1;
        } else if c == u16::from(b']') {
            depth -= 1;
            if depth == 0 {
                return Some(j);
            }
        }
        j += 1;
    }
    None
}

/// `/^[\p{L}_\\][\p{L}\p{N}_.\\]*$/u` (code points).
fn plain_name(s: &[u16]) -> bool {
    let mut first = true;
    for c in uo_text::utf16::code_points(s) {
        let m = uo_text::props::mask(c);
        let ok = if first {
            m & uo_text::props::LETTER != 0 || c == 0x5F || c == 0x5C
        } else {
            m & (uo_text::props::LETTER | uo_text::props::NUMBER) != 0 || matches!(c, 0x5F | 0x2E | 0x5C)
        };
        if !ok {
            return false;
        }
        first = false;
    }
    !first
}

/// `/^[\p{L}_][\p{L}\p{N}_]*$/u` (code points).
fn simple_col(s: &[u16]) -> bool {
    let mut first = true;
    for c in uo_text::utf16::code_points(s) {
        let m = uo_text::props::mask(c);
        let ok = if first {
            m & uo_text::props::LETTER != 0 || c == 0x5F
        } else {
            m & (uo_text::props::LETTER | uo_text::props::NUMBER) != 0 || c == 0x5F
        };
        if !ok {
            return false;
        }
        first = false;
    }
    !first
}

// ------------------------------------------------------------------ translator

/// The translator with its reusable buffers (`toLocal`, `fromLocal`, the name lookups).
#[derive(Debug, Default)]
pub struct Translator {
    src: Vec<u16>,
    toks: Vec<Tok>,
    toks2: Vec<Tok>,
    items: Vec<SItem>,
    /// Upper-cased stripped names of doubled prefixes ([`Canon::Dyn`]).
    dyn_buf: String,
    fold: String,
    fold2: String,
    u16a: Vec<u16>,
    u16b: Vec<u16>,
    u16c: Vec<u16>,
    /// `matchBracket` results of the current text once a scan failed (see [`Translator::bracket`]); empty = none.
    bmemo: Vec<u32>,
    close_of: Vec<u32>,
    stack: Vec<u32>,
    frames: Vec<OpFrame>,
    pframes: Vec<PFrame>,
    params: Vec<(u32, u32, u32)>,
    param_units: Vec<u16>,
    src_d: Dialect,
    tgt_d: Dialect,
}

#[derive(Clone, Copy, Debug)]
struct OpFrame {
    op: &'static str,
    out_start: u32,
    close: u32,
    fn_at: u32,
}

#[derive(Clone, Copy, Debug)]
struct PFrame {
    lambda: bool,
    depth: i64,
    arg: u32,
}

impl Translator {
    /// A translator (buffers grow on the first calls and are kept).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `toLocal(canonical, opts)`: canonical (or file form) → the text in the user's language, as Excel's formula
    /// bar shows it, appended to `out`. A leading `=` is kept.
    pub fn to_local_into(&mut self, canonical: JsStr<'_>, opts: &SyntaxOptions<'_>, out: &mut Vec<u16>) {
        self.load(canonical);
        let mut src = core::mem::take(&mut self.src_d);
        let mut tgt = core::mem::take(&mut self.tgt_d);
        src.canonical(opts.r1c1);
        tgt.resolve(opts);
        self.tokenize(&src, opts.names);
        self.rewrite_operators();
        self.scope_params(false);
        self.render(&tgt, false, opts.in_table, out);
        self.src_d = src;
        self.tgt_d = tgt;
    }

    /// `fromLocal(local, opts)`: the text in the user's language → canonical (`file_prefixes`: the OOXML form),
    /// appended to `out`.
    pub fn from_local_into(&mut self, local: JsStr<'_>, opts: &SyntaxOptions<'_>, out: &mut Vec<u16>) {
        self.load(local);
        let mut src = core::mem::take(&mut self.src_d);
        let mut tgt = core::mem::take(&mut self.tgt_d);
        src.resolve(opts);
        tgt.canonical(opts.r1c1);
        self.tokenize(&src, opts.names);
        self.scope_params(opts.file_prefixes);
        self.render(&tgt, opts.file_prefixes, opts.in_table, out);
        self.src_d = src;
        self.tgt_d = tgt;
    }

    /// [`Self::to_local_into`] into a new string.
    pub fn to_local(&mut self, canonical: JsStr<'_>, opts: &SyntaxOptions<'_>) -> JsString {
        let mut out = Vec::new();
        self.to_local_into(canonical, opts, &mut out);
        JsString::from_utf16(&out)
    }

    /// [`Self::from_local_into`] into a new string.
    pub fn from_local(&mut self, local: JsStr<'_>, opts: &SyntaxOptions<'_>) -> JsString {
        let mut out = Vec::new();
        self.from_local_into(local, opts, &mut out);
        JsString::from_utf16(&out)
    }

    fn load(&mut self, s: JsStr<'_>) {
        self.src.clear();
        self.src.extend(s.units());
        self.toks.clear();
        self.items.clear();
        self.dyn_buf.clear();
        self.bmemo.clear();
    }

    /// `fnLookup(d, word)`: the folded word, else the folded word without diacritics.
    pub(crate) fn fn_lookup(&mut self, lang: &FormulaLanguage, word: &[u16]) -> Option<&'static str> {
        fn_lookup_raw(lang, word, &mut self.fold, &mut self.fold2)
    }

    /// [`Self::fn_lookup`] of the input span `[a, b)`.
    fn fn_lookup_at(&mut self, lang: &FormulaLanguage, a: usize, b: usize) -> Option<&'static str> {
        fn_lookup_raw(lang, &self.src[a..b], &mut self.fold, &mut self.fold2)
    }

    /// `catalogEntry(word)` → `stripFilePrefix(word)` of the input span `[a, b)` as a [`Canon`] (static when it is
    /// the entry's own name).
    fn catalog_canon_at(&mut self, a: usize, b: usize) -> Option<Canon> {
        upper_utf8_into(JsStr::Utf16(&self.src[a..b]), &mut self.fold);
        let once = strip_upper_prefix(&self.fold);
        if let Some(e) = entry_exact(once) {
            return Some(Canon::Static(e.name));
        }
        // `catalogEntry(canon)` strips again (a doubled prefix): the token keeps the once-stripped name
        if entry_exact(strip_upper_prefix(once)).is_some() {
            let s = self.dyn_buf.len() as u32;
            self.dyn_buf.push_str(once);
            return Some(Canon::Dyn(s, self.dyn_buf.len() as u32));
        }
        None
    }

    fn canon_str(&self, c: Canon) -> &str {
        match c {
            Canon::Static(s) => s,
            Canon::Dyn(a, b) => &self.dyn_buf[a as usize..b as usize],
        }
    }
}

/// `s.toUpperCase()` of UTF-16 units into `out` (cleared first; a lone surrogate maps to itself, as V8).
fn upper_units_into(s: &[u16], out: &mut Vec<u16>) {
    out.clear();
    let mut i = 0;
    while i < s.len() {
        let u = s[i];
        if u < 0x80 {
            out.push(u16::from((u as u8).to_ascii_uppercase()));
            i += 1;
            continue;
        }
        let (cp, w) = uo_text::utf16::decode_at(s, i).unwrap_or((u32::from(u), 1));
        match uo_text::case::upper_mapping(cp) {
            uo_text::case::CaseMapping::Same => out.extend_from_slice(&s[i..i + w]),
            uo_text::case::CaseMapping::One(m) => uo_text::utf16::push(out, m),
            uo_text::case::CaseMapping::Many(units) => out.extend_from_slice(units),
        }
        i += w;
    }
}

/// `fnLookup(d, word)` with explicit buffers.
fn fn_lookup_raw(lang: &FormulaLanguage, word: &[u16], fold: &mut String, fold2: &mut String) -> Option<&'static str> {
    upper_utf8_into(JsStr::Utf16(word), fold);
    if let Some(c) = lang.fn_in_get(fold) {
        return Some(c);
    }
    if fold.is_ascii() {
        return None;
    }
    strip_marks(fold, fold2);
    lang.fn_in_get(fold2)
}

/// `s.normalize('NFD').replace(/\p{M}/gu, '').normalize('NFC')` of a UTF-8 text into `out`.
fn strip_marks(s: &str, out: &mut String) {
    let u: Vec<u16> = s.encode_utf16().collect();
    let d = uo_text::nfc::nfd(&u);
    let kept: Vec<u16> = {
        let mut v = Vec::with_capacity(d.len());
        let mut i = 0;
        while i < d.len() {
            let (cp, w) = uo_text::utf16::decode_at(&d, i).unwrap_or((u32::from(d[i]), 1));
            if uo_text::props::mask(cp) & uo_text::props::MARK == 0 {
                v.extend_from_slice(&d[i..i + w]);
            }
            i += w;
        }
        v
    };
    let c = uo_text::nfc::nfc(&kept);
    out.clear();
    out.push_str(&String::from_utf16_lossy(&c));
}

mod tokenize;
