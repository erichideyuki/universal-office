//! ENTRY of a cell as Excel 365 does it: the text typed (in the user's language and region) becomes a typed
//! constant, a formula, or is refused; the number format the entry applies; the EDIT text of a constant.
//! 1:1 port of `sheet/src/entry.ts`, `entry-format.ts`, `edit-text.ts` and `formula-locale.ts` (the rules were
//! measured in Excel pt-BR; the TS headers list them).
//!
//! The locale is the C1 host hook: [`FormulaLocale`] carries the language of formulas and the
//! [`uo_calc::ValueLocale`] the shell derived. FORMULA text goes through a [`FormulaSyntax`] (the TS `fromLocal` of
//! `calc/src/i18n/translate.ts`, which belongs to the calc's i18n frontier): [`CanonicalSyntax`] covers the canonical
//! syntax (en-US and any region with `.` decimal and `,` list), the pt-BR names need the A5 translator (decision
//! C3a-8); until it is wired a pt-BR formula entry answers [`Entry::Unsupported`].

use uo_calc::ast::{Ast, BinOp, Node, NodeId};
use uo_calc::coerce::parse_date_time_parts;
use uo_calc::fnreg::{CatPrefix, catalog_entry};
use uo_calc::locale::{DateOrder, ValueLocale};
use uo_calc::parse::{ParseOptions, number_literal_value, parse_formula};
use uo_calc::print::{PrintOptions, print_formula};
use uo_calc::strings::JsStr;
use uo_calc::value::ErrorCode;
use uo_jsnum::math;
use uo_text::utf16::{code_points, from_str};

fn u16s(s: &str) -> Vec<u16> {
    from_str(s)
}

fn is_digit(u: u16) -> bool {
    (48..=57).contains(&u)
}

// ---------------------------------------------------------------- formula-locale.ts

/// Language of formulas (`FormulaLang`): names of functions, errors, booleans.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FormulaLang {
    /// pt-BR (the measured table).
    PtBr,
    /// en-US (canonical).
    EnUs,
}

/// `FormulaLocale`: the language of formulas plus the REGION (decimal and list separators).
#[derive(Clone, Debug)]
pub struct FormulaLocale {
    /// Language.
    pub lang: FormulaLang,
    /// BCP 47 region tag.
    pub region: String,
    /// Decimal separator of the region.
    pub decimal: String,
    /// List separator (`;` when the decimal is `,`).
    pub list: String,
    /// Value conventions (text ↔ number/date, TRUE…).
    pub values: ValueLocale,
}

impl FormulaLocale {
    /// `formulaLocale('pt-BR')`.
    #[must_use]
    pub fn pt_br() -> Self {
        Self::for_tag("pt-BR", None, None)
    }

    /// `formulaLocale('en-US')`.
    #[must_use]
    pub fn en_us() -> Self {
        Self::for_tag("en-US", None, None)
    }

    /// `formulaLocale(tag)`: pt-BR for any `pt*`, en-US for the rest; the decimal is the region's (the shell's
    /// `Intl` answer, `regional_decimal`), the list `;` when it is `,`. `region_values`: the value conventions the
    /// shell derived for a region other than the language's own (`valueLocaleFor`).
    #[must_use]
    pub fn for_tag(tag: &str, regional_decimal: Option<&str>, region_values: Option<ValueLocale>) -> Self {
        let region = uo_jsnum::text::trim(tag).to_owned();
        let lower = region.to_ascii_lowercase();
        let pt = lower.starts_with("pt") && lower.as_bytes().get(2).is_none_or(|b| !(b.is_ascii_alphanumeric() || *b == b'_'));
        let lang = if pt { FormulaLang::PtBr } else { FormulaLang::EnUs };
        let (base, lang_id) = if pt { (ValueLocale::pt_br(), "pt-br") } else { (ValueLocale::en_us(), "en-us") };
        let decimal = regional_decimal.map_or_else(|| base.decimal.as_js().to_string_lossy(), str::to_owned);
        let list = if decimal == "," { ";" } else { "," }.to_owned();
        let mut values = base.clone();
        if lower != lang_id
            && let Some(r) = region_values
        {
            values = ValueLocale {
                true_text: base.true_text.clone(),
                false_text: base.false_text.clone(),
                general_name: base.general_name.clone(),
                format_letters: base.format_letters.clone(),
                ..r
            };
        }
        if values.decimal.as_js().to_string_lossy() != decimal {
            let group = if decimal == "," { "." } else { "," };
            values.decimal = uo_calc::strings::JsString::from_str_js(&decimal);
            values.group = uo_calc::strings::JsString::from_str_js(group);
        }
        FormulaLocale { lang, region, decimal, list, values }
    }

    fn decimal_units(&self) -> Vec<u16> {
        u16s(&self.decimal)
    }
}

// ---------------------------------------------------------------- entry-format.ts

/// Categories of a number format (`FmtCategory`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FmtCategory {
    /// General.
    General,
    /// Number.
    Number,
    /// Currency.
    Currency,
    /// Percentage.
    Percent,
    /// Scientific.
    Scientific,
    /// Fraction.
    Fraction,
    /// Date.
    Date,
    /// Time.
    Time,
    /// Text.
    Text,
}

/// The format to apply: numFmtId (+ formatCode when Excel writes it with the id, as currency 6/8).
#[derive(Clone, PartialEq, Debug)]
pub struct NumFmtHint {
    /// numFmtId.
    pub id: u32,
    /// Category.
    pub cat: FmtCategory,
    /// formatCode.
    pub code: Option<String>,
}

const fn hint(id: u32, cat: FmtCategory) -> NumFmtHint {
    NumFmtHint { id, cat, code: None }
}

/// `FMT`: the formats entries apply.
pub mod fmt {
    use super::{FmtCategory as C, NumFmtHint, hint};
    /// `#,##0` (3).
    pub const THOUSANDS: NumFmtHint = hint(3, C::Number);
    /// `#,##0.00` (4).
    pub const THOUSANDS2: NumFmtHint = hint(4, C::Number);
    /// `0.00E+00` (11).
    pub const SCI: NumFmtHint = hint(11, C::Scientific);
    /// `0%` (9).
    pub const PCT: NumFmtHint = hint(9, C::Percent);
    /// `0.00%` (10).
    pub const PCT2: NumFmtHint = hint(10, C::Percent);
    /// `# ?/?` (12).
    pub const FRAC: NumFmtHint = hint(12, C::Fraction);
    /// Short date (14).
    pub const DATE: NumFmtHint = hint(14, C::Date);
    /// d-mmm-yy (15).
    pub const D_MMM_YY: NumFmtHint = hint(15, C::Date);
    /// d-mmm (16).
    pub const D_MMM: NumFmtHint = hint(16, C::Date);
    /// mmm-yy (17).
    pub const MMM_YY: NumFmtHint = hint(17, C::Date);
    /// Date and time (22).
    pub const DATE_TIME: NumFmtHint = hint(22, C::Date);
    /// h:mm AM/PM (18).
    pub const HM_AM_PM: NumFmtHint = hint(18, C::Time);
    /// h:mm (20).
    pub const HM: NumFmtHint = hint(20, C::Time);
    /// h:mm:ss (21).
    pub const HMS: NumFmtHint = hint(21, C::Time);
    /// [h]:mm:ss (46).
    pub const ELAPSED: NumFmtHint = hint(46, C::Time);
    /// mm:ss.0 (47).
    pub const MS_TENTH: NumFmtHint = hint(47, C::Time);
}

/// `currencyHint`: the formatCode Excel writes with ids 6 (integer) and 8 (2 decimals).
#[must_use]
pub fn currency_hint(symbol: &str, decimals: bool) -> NumFmtHint {
    let sym: String = symbol.chars().filter(|&c| c != '"').collect();
    let q = format!("\"{sym}\"");
    let dec = if decimals { ".00" } else { "" };
    // pt-BR measured (SaveAs): "R$"\ #,##0;[Red]\-"R$"\ #,##0. en-US: the ECMA builtins 6/8
    let code = if symbol == "$" {
        format!("{q}#,##0{dec}_);[Red]\\({q}#,##0{dec}\\)")
    } else {
        format!("{q}\\ #,##0{dec};[Red]\\-{q}\\ #,##0{dec}")
    };
    NumFmtHint { id: if decimals { 8 } else { 6 }, cat: FmtCategory::Currency, code: Some(code) }
}

fn currency_char(c: char) -> bool {
    matches!(c, '$' | '€' | '£' | '¥')
}

fn currency_text(lit: &str) -> bool {
    lit.chars().any(currency_char) || lit.contains("R$")
}

/// `firstSection`.
fn first_section(code: &str) -> &str {
    let mut q = false;
    let mut b = false;
    let mut it = code.char_indices();
    while let Some((i, ch)) = it.next() {
        if ch == '\\' && !q {
            it.next();
            continue;
        }
        if ch == '"' {
            q = !q;
        } else if !q && ch == '[' {
            b = true;
        } else if !q && ch == ']' {
            b = false;
        } else if !q && !b && ch == ';' {
            return code.get(..i).unwrap_or(code);
        }
    }
    code
}

fn is_time_bracket(inner: &str) -> bool {
    // /^(h+|m+|s+)$/i
    let mut it = inner.chars().map(|c| c.to_ascii_lowercase());
    match it.next() {
        Some(f @ ('h' | 'm' | 's')) => it.all(|c| c == f),
        _ => false,
    }
}

/// `formatCategory`: the category of a format code (the first section), as the Format Cells box groups them.
#[must_use]
pub fn format_category(code: Option<&str>) -> FmtCategory {
    let Some(code) = code.filter(|c| !c.is_empty()) else { return FmtCategory::General };
    let sec = first_section(code);
    let chars: Vec<char> = sec.chars().collect();
    let (mut date, mut time, mut month, mut sci, mut pct, mut frac, mut cur, mut text, mut digits) =
        (false, false, false, false, false, false, false, false, false);
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '"' {
            let j = chars.iter().skip(i + 1).position(|&c| c == '"').map(|k| i + 1 + k);
            let lit: String = chars.get(i + 1..j.unwrap_or(chars.len())).unwrap_or(&[]).iter().collect();
            if currency_text(&lit) {
                cur = true;
            }
            i = j.unwrap_or(chars.len()) + 1;
            continue;
        }
        if matches!(ch, '\\' | '_' | '*') {
            if ch == '\\' && chars.get(i + 1).is_some_and(|&c| currency_char(c)) {
                cur = true;
            }
            i += 2;
            continue;
        }
        if ch == '[' {
            let j = chars.iter().skip(i).position(|&c| c == ']').map(|k| i + k);
            let inner: String = chars.get(i + 1..j.unwrap_or(chars.len())).unwrap_or(&[]).iter().collect();
            if is_time_bracket(&inner) {
                time = true;
            } else if inner.starts_with('$') && inner.chars().skip(1).any(|c| c != '-') && inner.chars().nth(1).is_some_and(|c| c != '-') {
                // /^\$[^-]+/
                cur = true;
            }
            i = j.unwrap_or(chars.len()) + 1;
            continue;
        }
        match ch.to_ascii_lowercase() {
            'y' | 'd' => date = true,
            'h' | 's' => time = true,
            'm' => month = true,
            'e' if matches!(chars.get(i + 1), Some('+' | '-')) => sci = true,
            '%' => pct = true,
            '/' => frac = true,
            '@' => text = true,
            '$' | '€' | '£' | '¥' => cur = true,
            '0' | '#' | '?' => digits = true,
            'g' => {
                let rest: String = chars.iter().skip(i).take(7).collect();
                if rest.eq_ignore_ascii_case("general") {
                    return FmtCategory::General;
                }
            }
            _ => {}
        }
        i += 1;
    }
    if date || (month && !time) {
        return FmtCategory::Date;
    }
    if time || month {
        return FmtCategory::Time;
    }
    if text && !digits {
        return FmtCategory::Text;
    }
    if sci {
        return FmtCategory::Scientific;
    }
    if pct {
        return FmtCategory::Percent;
    }
    if frac && digits {
        return FmtCategory::Fraction;
    }
    if cur {
        return FmtCategory::Currency;
    }
    if digits { FmtCategory::Number } else { FmtCategory::General }
}

/// `shouldApplyFormat`: does the entry's implicit format replace the cell's current one?
#[must_use]
pub fn should_apply_format(current: FmtCategory, hint: &NumFmtHint) -> bool {
    use FmtCategory as C;
    if current == C::General {
        return true;
    }
    if !matches!(current, C::Percent | C::Scientific | C::Fraction | C::Date | C::Time) {
        return false;
    }
    if matches!(hint.cat, C::Number | C::General | C::Text) {
        return false;
    }
    hint.cat != current
}

/// A reference's format: the TS `FormatLookup` (`sheet` text of the formula or `None` for the cell's own sheet;
/// 0-based row and column, possibly fractional or negative as an R1C1 offset can be).
pub type FormatLookup<'a> = &'a dyn Fn(Option<&[u16]>, f64, f64) -> Option<(u32, String)>;

/// Depth `inferFormulaFormat` follows (the TS recurses without a limit; a deeper tree infers nothing).
const INFER_DEPTH: u32 = 512;

/// `inferFormulaFormat`: the format a formula gives a General cell (`None` = none). `row`/`col`: the formula's
/// cell (0-based).
#[must_use]
pub fn infer_formula_format(ast: &Ast, node: NodeId, row: u32, col: u32, format_at: FormatLookup<'_>) -> Option<NumFmtHint> {
    infer(ast, node, f64::from(row), f64::from(col), format_at, 0)
}

fn infer(ast: &Ast, n: NodeId, row: f64, col: f64, at: FormatLookup<'_>, depth: u32) -> Option<NumFmtHint> {
    if depth > INFER_DEPTH {
        return None;
    }
    let go = |m: NodeId| infer(ast, m, row, col, at, depth + 1);
    let reference = |sheet: u32, rp: uo_calc::ast::Part, cp: uo_calc::ast::Part| -> Option<NumFmtHint> {
        let rel = |p: uo_calc::ast::Part, base: f64| ast.part(p).map_or(0.0, |p| if p.abs { p.n } else { base + p.n });
        let (r, c) = (rel(rp, row), rel(cp, col));
        let name = ast.spec(sheet).map(|s| ast.str(s.sheet).to_utf16());
        let (id, code) = at(name.as_deref(), r, c)?;
        let cat = format_category(Some(&code));
        (cat != FmtCategory::General).then_some(NumFmtHint { id, cat, code: Some(code) })
    };
    match *ast.node(n) {
        Node::Cell { sheet, row: rp, col: cp } => reference(sheet, rp, cp),
        Node::Area { sheet, r1, c1, .. } => reference(sheet, r1, c1),
        Node::Paren(e) => go(e),
        Node::Unary { op, e } => (op != uo_calc::ast::UnaryOp::Percent).then(|| go(e)).flatten(),
        Node::Bin { op, a, b } => match op {
            BinOp::Range => go(a),
            BinOp::Add | BinOp::Sub => {
                let (x, y) = (go(a), go(b));
                // date ± date = General (difference in days; sum of dates)
                if x.as_ref().is_some_and(|h| h.cat == FmtCategory::Date) && y.as_ref().is_some_and(|h| h.cat == FmtCategory::Date) {
                    return None;
                }
                x.or(y)
            }
            _ => None,
        },
        Node::Fn { name, args, .. } => {
            let nm = ast.fn_name(name);
            let is = |s: &str| nm.eq_str(s);
            if is("TODAY") || is("DATE") {
                return Some(fmt::DATE);
            }
            if is("NOW") {
                return Some(fmt::DATE_TIME);
            }
            if is("TIME") {
                return Some(fmt::HM_AM_PM);
            }
            let kids = ast.kids(args);
            if is("SINGLE") {
                return kids.first().and_then(|&k| go(k));
            }
            if is("ROUND") {
                return kids.first().and_then(|&k| go(k));
            }
            if is("SUM") || is("AVERAGE") || is("MIN") || is("MAX") {
                return kids.iter().find_map(|&k| go(k));
            }
            None
        }
        _ => None,
    }
}

// ---------------------------------------------------------------- entry.ts

/// A typed constant.
#[derive(Clone, PartialEq, Debug)]
pub enum EntryValue {
    /// Number.
    Number(f64),
    /// Text (UTF-16).
    Text(Vec<u16>),
    /// Boolean.
    Bool(bool),
    /// Error.
    Error(ErrorCode),
}

/// What the typed text became (`Entry`).
#[derive(Debug)]
pub enum Entry {
    /// `""` clears.
    Clear,
    /// A constant (+ the format it applies, + the text prefix `'`).
    Value {
        /// The value.
        value: EntryValue,
        /// Format to apply.
        fmt: Option<NumFmtHint>,
        /// `'` text prefix.
        quote_prefix: bool,
    },
    /// A formula: `text` = file form (canonical, prefixes, no `=`); the tree is relative to the cell.
    Formula {
        /// File text.
        text: Vec<u16>,
        /// The tree.
        ast: Box<Ast>,
        /// Its root.
        root: NodeId,
    },
    /// Refused (Excel does not accept the entry).
    Invalid {
        /// Message in the formula language.
        message: &'static str,
    },
    /// A formula whose language this build cannot translate yet (pt-BR names without the calc's i18n; C3a-8).
    Unsupported,
}

/// Context of an entry (`EntryContext`).
#[derive(Clone, Copy, Debug)]
pub struct EntryContext<'a> {
    /// Locale.
    pub locale: &'a FormulaLocale,
    /// Category of the cell's current format.
    pub current: FmtCategory,
    /// Cell row (0-based): relative references are from it.
    pub row: u32,
    /// Cell column.
    pub col: u32,
    /// 1904 date system.
    pub date1904: bool,
    /// The year a date typed without one takes (`new Date().getFullYear()`: the host's clock).
    pub year: f64,
    /// Defined names of the workbook (win over a function as a value).
    pub names: &'a [&'a str],
    /// Table the cell is in (`[@Col]`).
    pub in_table: Option<&'a str>,
}

/// The answer of [`FormulaSyntax::arity`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arity {
    /// Accepted (or unknown function).
    Ok,
    /// Too few arguments.
    Few,
    /// Too many arguments.
    Many,
}

/// The formula translator of the calc's i18n (`fromLocal` with `filePrefixes`, the catalogue's `functionKind` and
/// `acceptsArgCount`), as a hook so the grid does not own the language tables.
pub trait FormulaSyntax {
    /// `fromLocal(input, { ...locale.syntax, filePrefixes: true, names, inTable })`: local text → canonical file
    /// text; `None` = this translator does not handle the locale.
    fn to_canonical(&self, local: &[u16], ctx: &EntryContext<'_>) -> Option<Vec<u16>>;
    /// `functionKind(name) === 'reserved'`.
    fn is_reserved(&self, _canonical_name: &[u16]) -> bool {
        false
    }
    /// `acceptsArgCount(catalogEntry(name), n)` (`Ok` when the name is not in the catalogue).
    fn arity(&self, canonical_name: &[u16], n_args: usize) -> Arity;
}

/// The canonical syntax (`.` decimal, `,` list): function names upper-cased and given their file prefix
/// (`_xlfn.`), booleans and errors upper-cased, everything else as typed. Arity from the registered functions'
/// minimum and maximum (the catalogue's step counts and small-count lists belong to the calc's i18n: C3a-8).
#[derive(Clone, Copy, Debug, Default)]
pub struct CanonicalSyntax;

fn is_ident_start(cp: u32) -> bool {
    cp == 95 || cp == 92 || uo_text::props::is_letter(cp)
}

fn is_ident_part(cp: u32) -> bool {
    is_ident_start(cp) || cp == 46 || uo_text::props::is_number(cp)
}

impl FormulaSyntax for CanonicalSyntax {
    fn to_canonical(&self, local: &[u16], ctx: &EntryContext<'_>) -> Option<Vec<u16>> {
        if ctx.locale.lang != FormulaLang::EnUs || ctx.locale.decimal != "." {
            return None;
        }
        let mut out: Vec<u16> = Vec::with_capacity(local.len() + 8);
        let n = local.len();
        let mut i = 0;
        let at = |k: usize| local.get(k).copied();
        while i < n {
            let u = local[i];
            if u == 34 || u == 39 {
                // text / quoted sheet name: copied (doubled quotes)
                let q = u;
                out.push(u);
                i += 1;
                while i < n {
                    out.push(local[i]);
                    if local[i] == q {
                        if at(i + 1) == Some(q) {
                            out.push(q);
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            if u == 91 {
                // a structured reference needs the translator's `[@Col]` / `[[#This Row],…]` rewriting
                return None;
            }
            if let Some((cp, w)) = uo_text::utf16::decode_at(local, i)
                && is_ident_start(cp)
            {
                // an identifier: a function (followed by `(`), a boolean, an error-free name or a reference
                let prev_ident =
                    i > 0 && uo_text::utf16::decode_before(local, i).is_some_and(|(p, _)| is_ident_part(p) || p == 33 || p == 36);
                let mut j = i + w;
                while let Some((c2, w2)) = uo_text::utf16::decode_at(local, j) {
                    if !is_ident_part(c2) {
                        break;
                    }
                    j += w2;
                }
                let word = &local[i..j];
                if !prev_ident && at(j) == Some(40) {
                    let up = uo_text::case::to_uppercase(word).into_owned();
                    // LET/LAMBDA parameters get `_xlpm.` / `_xlop.` in the file: the translator's scoping pass
                    if up == u16s("LET") || up == u16s("LAMBDA") {
                        return None;
                    }
                    let stripped = uo_calc::fnreg::strip_file_prefix(JsStr::Utf16(word));
                    let already = up.starts_with(&u16s("_XLFN.")) || up.starts_with(&u16s("_XLWS.")) || up.starts_with(&u16s("_XLUDF."));
                    match catalog_entry(JsStr::Utf16(&stripped)) {
                        Some(ix) if !already => {
                            let e = ix.entry();
                            out.extend(e.catalog.text().unwrap_or("").encode_utf16());
                            out.extend(e.name.encode_utf16());
                        }
                        Some(_) => out.extend_from_slice(&up),
                        None => out.extend_from_slice(word),
                    }
                } else if !prev_ident && word.len() == 4 && uo_text::case::to_uppercase(word).as_ref() == u16s("TRUE").as_slice()
                    || !prev_ident && word.len() == 5 && uo_text::case::to_uppercase(word).as_ref() == u16s("FALSE").as_slice()
                {
                    out.extend(uo_text::case::to_uppercase(word).iter());
                } else {
                    out.extend_from_slice(word);
                }
                i = j;
                continue;
            }
            out.push(u);
            i += 1;
        }
        Some(out)
    }

    fn arity(&self, canonical_name: &[u16], n_args: usize) -> Arity {
        let Some(ix) = uo_calc::fnreg::lookup_fn(JsStr::Utf16(canonical_name)) else { return Arity::Ok };
        let Some(def) = ix.entry().def else { return Arity::Ok };
        let n = n_args as u64;
        if n < u64::from(def.min_args) {
            Arity::Few
        } else if n > u64::from(def.max_args) {
            Arity::Many
        } else {
            Arity::Ok
        }
    }
}

struct Msgs {
    syntax: &'static str,
    few: &'static str,
    many: &'static str,
    reserved: &'static str,
}

fn msgs(lang: FormulaLang) -> Msgs {
    match lang {
        FormulaLang::PtBr => Msgs {
            syntax: "Há um problema com esta fórmula.",
            few: "Você inseriu poucos argumentos para esta função.",
            many: "Você inseriu muitos argumentos para esta função.",
            reserved: "Essa função não é válida.",
        },
        FormulaLang::EnUs => Msgs {
            syntax: "There's a problem with this formula.",
            few: "You've entered too few arguments for this function.",
            many: "You've entered too many arguments for this function.",
            reserved: "That function isn't valid.",
        },
    }
}

/// `parseEntry`: analyzes the text typed in a cell.
#[must_use]
pub fn parse_entry(input: &[u16], ctx: &EntryContext<'_>, syntax: &dyn FormulaSyntax) -> Entry {
    if input.is_empty() {
        return Entry::Clear;
    }
    let value = |v: EntryValue, quote_prefix: bool| Entry::Value { value: v, fmt: None, quote_prefix };
    if ctx.current == FmtCategory::Text {
        return value(EntryValue::Text(input.to_vec()), false);
    }
    if input[0] == 39 {
        return value(EntryValue::Text(input[1..].to_vec()), true);
    }
    if input[0] == 61 && input.len() > 1 {
        return parse_formula_entry(input, ctx, syntax);
    }
    if let Some(c) = parse_constant(input, ctx) {
        return c;
    }
    if (input[0] == 43 || input[0] == 45) && input.len() > 1 {
        let mut f = Vec::with_capacity(input.len() + 1);
        f.push(61);
        f.extend_from_slice(input);
        let e = parse_formula_entry(&f, ctx, syntax);
        if matches!(e, Entry::Formula { .. } | Entry::Unsupported) {
            return e;
        }
    }
    value(EntryValue::Text(input.to_vec()), false)
}

const TYPABLE_ERRORS: [(&str, &str, ErrorCode); 8] = [
    ("#NULL!", "#NULO!", ErrorCode::Null),
    ("#DIV/0!", "#DIV/0!", ErrorCode::Div0),
    ("#VALUE!", "#VALOR!", ErrorCode::Value),
    ("#REF!", "#REF!", ErrorCode::Ref),
    ("#NAME?", "#NOME?", ErrorCode::Name),
    ("#NUM!", "#NÚM!", ErrorCode::Num),
    ("#N/A", "#N/D", ErrorCode::NA),
    ("#GETTING_DATA", "#OBTENDO_DADOS", ErrorCode::GettingData),
];

/// `bare`: NFD without marks, upper case.
fn bare(units: &[u16]) -> Vec<u16> {
    let d = uo_text::nfc::nfd(units);
    let mut kept: Vec<u16> = Vec::with_capacity(d.len());
    for cp in code_points(&d) {
        if !uo_text::props::is_mark(cp) {
            uo_text::utf16::push(&mut kept, cp);
        }
    }
    uo_text::case::to_uppercase(&kept).into_owned()
}

fn upper(u: &[u16]) -> Vec<u16> {
    uo_text::case::to_uppercase(u).into_owned()
}

fn parse_constant(input: &[u16], ctx: &EntryContext<'_>) -> Option<Entry> {
    let lang = ctx.locale.lang;
    let up = upper(input);
    let (t, f) = if lang == FormulaLang::PtBr { ("VERDADEIRO", "FALSO") } else { ("TRUE", "FALSE") };
    let val = |v: EntryValue| Some(Entry::Value { value: v, fmt: None, quote_prefix: false });
    if up == upper(&u16s(t)) {
        return val(EntryValue::Bool(true));
    }
    if up == upper(&u16s(f)) {
        return val(EntryValue::Bool(false));
    }
    if input[0] == 35 {
        let b = bare(input);
        for (en, pt, code) in TYPABLE_ERRORS {
            let local = if lang == FormulaLang::PtBr { pt } else { en };
            if bare(&u16s(local)) == b {
                return val(EntryValue::Error(code));
            }
        }
        return None;
    }
    if let Some(n) = parse_number_entry(input, ctx) {
        return Some(n);
    }
    let s = trim_spaces(input);
    // "a/b" outside a General/date/time cell: the fraction (measured: "1/2" in 0% → 0,5 "# ?/?")
    if !matches!(ctx.current, FmtCategory::General | FmtCategory::Date | FmtCategory::Time)
        && let Some((neg, num, den)) = simple_fraction(s)
        && den != 0.0
        && den <= 32767.0
        && num <= 32767.0
    {
        let v = num / den;
        return Some(Entry::Value { value: EntryValue::Number(if neg { -v } else { v }), fmt: Some(fmt::FRAC), quote_prefix: false });
    }
    let values = &ctx.locale.values;
    let parts = parse_date_time_parts(JsStr::Utf16(s), values, ctx.date1904, ctx.year)?;
    let v = parts.date.unwrap_or(0.0) + parts.time.unwrap_or(0.0);
    let mut month_words: Vec<Vec<u16>> = Vec::new();
    for m in values.month_names.iter().chain(values.month_abbr.iter()) {
        month_words.push(m.as_js().to_utf16());
    }
    Some(Entry::Value {
        value: EntryValue::Number(v),
        fmt: Some(date_time_format(s, parts.date, parts.time, &month_words)),
        quote_prefix: false,
    })
}

fn trim_spaces(s: &[u16]) -> &[u16] {
    let a = s.iter().take_while(|&&u| u == 32).count();
    let rest = s.get(a..).unwrap_or(&[]);
    let b = rest.iter().rev().take_while(|&&u| u == 32).count();
    rest.get(..rest.len() - b).unwrap_or(&[])
}

/// `/^([+-]?)(\d+)\/(\d+)$/` → (negative, numerator, denominator).
fn simple_fraction(s: &[u16]) -> Option<(bool, f64, f64)> {
    let (neg, rest) = match s.first() {
        Some(45) => (true, &s[1..]),
        Some(43) => (false, &s[1..]),
        _ => (false, s),
    };
    let a = rest.iter().take_while(|&&u| is_digit(u)).count();
    if a == 0 || rest.get(a) != Some(&47) {
        return None;
    }
    let tail = &rest[a + 1..];
    if tail.is_empty() || !tail.iter().all(|&u| is_digit(u)) {
        return None;
    }
    Some((neg, digits_value(&rest[..a]), digits_value(tail)))
}

fn digits_value(d: &[u16]) -> f64 {
    uo_jsnum::parse::to_number(&String::from_utf16_lossy(d))
}

/// A parsing cursor over the typed number (the TS mutates `s`).
struct Num {
    s: Vec<u16>,
}

impl Num {
    fn trim(&mut self) {
        let t = trim_spaces(&self.s).to_vec();
        self.s = t;
    }
    fn starts_with(&self, p: &[u16]) -> bool {
        !p.is_empty() && self.s.starts_with(p)
    }
    fn drop_front(&mut self, n: usize) {
        self.s.drain(..n.min(self.s.len()));
        self.trim();
    }
    fn drop_back(&mut self, n: usize) {
        let k = self.s.len().saturating_sub(n);
        self.s.truncate(k);
        self.trim();
    }
}

/// A number typed (with %, currency, group, exponent, mixed fraction) → value + format.
fn parse_number_entry(input: &[u16], ctx: &EntryContext<'_>) -> Option<Entry> {
    let values = &ctx.locale.values;
    let mut st = Num { s: trim_spaces(input).to_vec() };
    if st.s.is_empty() {
        return None;
    }
    let (mut neg, mut signed, mut pct, mut cur, mut euro) = (false, false, false, false, false);
    let currency_sym = values.currency.as_js().to_utf16();
    // sign(): a second sign (or a sign inside parentheses) poisons the text
    macro_rules! sign {
        () => {
            if matches!(st.s.first(), Some(43 | 45)) {
                if signed {
                    st.s = vec![0];
                } else {
                    signed = true;
                    neg = st.s[0] == 45;
                    st.drop_front(1);
                }
            }
        };
    }
    macro_rules! currency {
        () => {
            if !cur && !currency_sym.is_empty() && st.starts_with(&currency_sym) {
                cur = true;
                st.drop_front(currency_sym.len());
            } else if !euro && st.s.first() == Some(&0x20AC) {
                euro = true;
                st.drop_front(1);
            }
        };
    }
    macro_rules! percent_prefix {
        () => {
            if !pct && st.s.first() == Some(&37) {
                pct = true;
                st.drop_front(1);
            }
        };
    }
    sign!();
    currency!();
    sign!();
    percent_prefix!();
    if st.s.first() == Some(&40) && st.s.last() == Some(&41) && !signed {
        neg = true;
        let inner = st.s.get(1..st.s.len() - 1).unwrap_or(&[]).to_vec();
        st.s = inner;
        st.trim();
        currency!();
        percent_prefix!();
    }
    if st.s.last() == Some(&37) && !pct {
        pct = true;
        st.drop_back(1);
    }
    if st.s.last() == Some(&0x20AC) && !euro && !cur {
        euro = true;
        st.drop_back(1);
    }
    if st.s.is_empty() || st.s.contains(&0) || (pct && (cur || euro)) {
        return None;
    }
    let s = &st.s;
    // mixed fraction "1 1/2" (numerator and denominator up to 32767, as the engine)
    if let Some((w, nu, de)) = mixed_fraction(s)
        && !pct
        && !cur
        && !euro
    {
        if de == 0.0 || de > 32767.0 || nu > 32767.0 {
            return None;
        }
        let v = w + nu / de;
        return Some(Entry::Value { value: EntryValue::Number(if neg { -v } else { v }), fmt: Some(fmt::FRAC), quote_prefix: false });
    }
    let group = values.group.as_js().to_utf16();
    let decimal = values.decimal.as_js().to_utf16();
    let m = match_number(s, &group, &decimal)?;
    if m.int.is_none() && m.frac.is_none_or(<[u16]>::is_empty) {
        return None;
    }
    let grouped = m.int.is_some_and(|i| contains_seq(i, &group));
    let has_dec = m.dec_present;
    let mut raw: Vec<u16> = Vec::new();
    let int_clean = m.int.map(|i| remove_seq(i, &group)).unwrap_or_default();
    if int_clean.is_empty() {
        raw.push(48);
    } else {
        raw.extend_from_slice(&int_clean);
    }
    if let Some(f) = m.frac
        && !f.is_empty()
    {
        raw.push(46);
        raw.extend_from_slice(f);
    }
    if let Some(e) = m.exp {
        raw.push(101);
        raw.extend_from_slice(e);
    }
    let (mut v, _) = number_literal_value(JsStr::Utf16(&raw))?;
    if pct {
        v /= 100.0;
    }
    if neg {
        v = -v;
    }
    let fmt = if pct {
        Some(if has_dec { fmt::PCT2 } else { fmt::PCT })
    } else if cur {
        Some(currency_hint(&values.currency.as_js().to_string_lossy(), has_dec))
    } else if m.exp.is_some() {
        Some(fmt::SCI)
    } else if grouped {
        Some(if has_dec { fmt::THOUSANDS2 } else { fmt::THOUSANDS })
    } else {
        None
    };
    Some(Entry::Value { value: EntryValue::Number(v), fmt, quote_prefix: false })
}

fn contains_seq(h: &[u16], n: &[u16]) -> bool {
    n.is_empty() || h.windows(n.len()).any(|w| w == n)
}

/// `h.split(n).join('')`.
fn remove_seq(h: &[u16], n: &[u16]) -> Vec<u16> {
    if n.is_empty() {
        return h.to_vec();
    }
    let mut out = Vec::with_capacity(h.len());
    let mut i = 0;
    while i < h.len() {
        if h[i..].starts_with(n) {
            i += n.len();
        } else {
            out.push(h[i]);
            i += 1;
        }
    }
    out
}

/// `/^(\d+) +(\d+)\/(\d+)$/` → (whole, numerator, denominator).
fn mixed_fraction(s: &[u16]) -> Option<(f64, f64, f64)> {
    let a = s.iter().take_while(|&&u| is_digit(u)).count();
    if a == 0 {
        return None;
    }
    let sp = s[a..].iter().take_while(|&&u| u == 32).count();
    if sp == 0 {
        return None;
    }
    let (num, den) = simple_fraction_parts(&s[a + sp..])?;
    Some((digits_value(&s[..a]), num, den))
}

fn simple_fraction_parts(s: &[u16]) -> Option<(f64, f64)> {
    let a = s.iter().take_while(|&&u| is_digit(u)).count();
    if a == 0 || s.get(a) != Some(&47) {
        return None;
    }
    let t = &s[a + 1..];
    if t.is_empty() || !t.iter().all(|&u| is_digit(u)) {
        return None;
    }
    Some((digits_value(&s[..a]), digits_value(t)))
}

struct NumMatch<'a> {
    int: Option<&'a [u16]>,
    dec_present: bool,
    frac: Option<&'a [u16]>,
    exp: Option<&'a [u16]>,
}

/// `^(\d+(?:G\d{3,})*)?(?:(D)(\d*))?(?:[eE]([+-]?\d+))?$` with the locale's group `G` and decimal `D`.
fn match_number<'a>(s: &'a [u16], group: &[u16], decimal: &[u16]) -> Option<NumMatch<'a>> {
    let mut i = 0;
    let run = |from: usize| s.get(from..).unwrap_or(&[]).iter().take_while(|&&u| is_digit(u)).count();
    let r = run(0);
    let mut int = None;
    if r > 0 {
        i = r;
        // (?:G\d{3,})*: each group takes every digit that follows (a shorter one leaves digits unmatched)
        while !group.is_empty() && s.get(i..).is_some_and(|t| t.starts_with(group)) && run(i + group.len()) >= 3 {
            i += group.len() + run(i + group.len());
        }
        int = Some(&s[..i]);
    }
    let mut dec_present = false;
    let mut frac = None;
    if !decimal.is_empty() && s.get(i..).is_some_and(|t| t.starts_with(decimal)) {
        dec_present = true;
        let d0 = i + decimal.len();
        let r = run(d0);
        frac = Some(&s[d0..d0 + r]);
        i = d0 + r;
    }
    let mut exp = None;
    if matches!(s.get(i), Some(69 | 101)) {
        let k = if matches!(s.get(i + 1), Some(43 | 45)) { i + 2 } else { i + 1 };
        let r = run(k);
        if r > 0 {
            exp = Some(&s[i + 1..k + r]);
            i = k + r;
        }
    }
    (i == s.len()).then_some(NumMatch { int, dec_present, frac, exp })
}

fn ci_eq(a: &[u16], b: &str) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.bytes())
            .all(|(&x, y)| x == u16::from(y) || x == u16::from(y.to_ascii_uppercase()) || x == u16::from(y.to_ascii_lowercase()))
}

fn is_ampm_word(w: &[u16]) -> bool {
    ci_eq(w, "AM") || ci_eq(w, "PM") || ci_eq(w, "A") || ci_eq(w, "P")
}

fn is_word_unit(u: u16) -> bool {
    u == 95 || is_digit(u) || (65..=90).contains(&u) || (97..=122).contains(&u)
}

/// `/\b(AM|PM|A|P)$/i.test(s)`.
fn ends_with_ampm(s: &[u16]) -> bool {
    for n in [2usize, 1] {
        if s.len() >= n {
            let suffix = &s[s.len() - n..];
            let ok = if n == 2 { ci_eq(suffix, "AM") || ci_eq(suffix, "PM") } else { ci_eq(suffix, "A") || ci_eq(suffix, "P") };
            let before = if s.len() > n { Some(s[s.len() - n - 1]) } else { None };
            // `\b`: a word character (the suffix's first letter) next to a non-word or the start
            if ok && before.is_none_or(|b| !is_word_unit(b)) {
                return true;
            }
        }
    }
    false
}

/// The format of a typed date/time by the shape of the text (`dateTimeFormat`).
fn date_time_format(s: &[u16], date: Option<f64>, time: Option<f64>, month_words: &[Vec<u16>]) -> NumFmtHint {
    // s.replace(/ *: */g, ':').split(/ +/).filter(...)
    let mut squeezed: Vec<u16> = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == 32 {
            let j = i + s[i..].iter().take_while(|&&u| u == 32).count();
            let touches = (i > 0 && s[i - 1] == 58) || s.get(j) == Some(&58);
            if !touches {
                squeezed.extend_from_slice(&s[i..j]);
            }
            i = j;
        } else {
            squeezed.push(s[i]);
            i += 1;
        }
    }
    let words: Vec<&[u16]> = squeezed.split(|&u| u == 32).collect::<Vec<_>>();
    // `split(/ +/)` yields empty strings only at the ends; the filter drops them anyway
    let time_words: Vec<&[u16]> = words.into_iter().filter(|w| w.contains(&58) || is_ampm_word(w)).collect();
    if date.is_some() && time.is_some() {
        return fmt::DATE_TIME;
    }
    if date.is_none() {
        let t = time_words.iter().find(|w| w.contains(&58)).copied().unwrap_or(&[]);
        let trimmed = uo_text::space::trim(s);
        if ends_with_ampm(trimmed) || time_words.iter().any(|w| is_ampm_word(w)) {
            return fmt::HM_AM_PM;
        }
        let segs: Vec<&[u16]> = t.split(|&u| u == 58).collect();
        if t.iter().any(|&u| !is_digit(u) && u != 58) {
            return fmt::MS_TENTH;
        }
        if segs.first().is_some_and(|s0| digits_value_js(s0) >= 24.0) {
            return fmt::ELAPSED;
        }
        return if segs.len() >= 3 { fmt::HMS } else { fmt::HM };
    }
    // only a date: with a month name or numeric
    let lower = uo_text::case::to_lowercase(s).into_owned();
    let words: Vec<&[u16]> =
        lower.split(|&u| uo_jsnum::text::is_js_space_unit(u) || u == 47 || u == 44 || u == 45).filter(|w| !w.is_empty()).collect();
    let named = words.iter().any(|w| month_words.iter().any(|mw| uo_text::case::to_lowercase(mw).as_ref() == *w));
    if named {
        if words.len() >= 3 {
            return fmt::D_MMM_YY;
        }
        return if words.first().is_some_and(|w| w.first().is_some_and(|&u| is_digit(u))) { fmt::D_MMM } else { fmt::MMM_YY };
    }
    if words.len() >= 3 {
        return fmt::DATE;
    }
    let w1_len = words.get(1).map_or(0, |w| w.len());
    if w1_len >= 3 {
        return fmt::MMM_YY;
    }
    let n1 = words.get(1).map_or(f64::NAN, |w| digits_value_js(w));
    let n0 = words.first().map_or(f64::NAN, |w| digits_value_js(w));
    if (1.0..=12.0).contains(&n1) && n0 <= 31.0 { fmt::D_MMM } else { fmt::MMM_YY }
}

/// `+word` (JS `Number(word)`).
fn digits_value_js(w: &[u16]) -> f64 {
    uo_jsnum::parse::to_number(&String::from_utf16_lossy(w))
}

// ------------------------------------------------------------------ formulas

fn parse_formula_entry(input: &[u16], ctx: &EntryContext<'_>, syntax: &dyn FormulaSyntax) -> Entry {
    let m = msgs(ctx.locale.lang);
    if bad_decimal(input, &ctx.locale.decimal_units()) {
        return Entry::Invalid { message: m.syntax };
    }
    let Some(mut canon) = syntax.to_canonical(input, ctx) else { return Entry::Unsupported };
    let po = ParseOptions { row: ctx.row, col: ctx.col, ..ParseOptions::default() };
    let mut ast = Ast::new();
    let mut root = parse_formula(&mut ast, JsStr::Utf16(&canon), &po).ok();
    if root.is_none()
        && let Some(closed) = auto_close(input)
    {
        let Some(c2) = syntax.to_canonical(&closed, ctx) else { return Entry::Unsupported };
        canon = c2;
        root = parse_formula(&mut ast, JsStr::Utf16(&canon), &po).ok();
    }
    let Some(root) = root else { return Entry::Invalid { message: m.syntax } };
    let mut bad: Option<&'static str> = None;
    ast.walk(root, |_, n| {
        if bad.is_some() {
            return;
        }
        // union (`;` inside parentheses) only of references: =SUM((1;2)) and =SUM((A1;2)) are refused (measured)
        if let Node::Bin { op: BinOp::Union, a, b } = *n
            && (is_constant(&ast, a) || is_constant(&ast, b))
        {
            bad = Some(m.syntax);
            return;
        }
        let Node::Fn { name, prefix, args, .. } = *n else { return };
        if prefix != 0 {
            // `_xludf.` prefix: a user-defined function
            let p = ast.str(uo_calc::ast::StrIx(prefix - 1)).to_utf16();
            if uo_text::case::to_uppercase(&p).windows(7).any(|w| w == u16s("_XLUDF.").as_slice()) {
                return;
            }
        }
        let nm = ast.fn_name(name).to_utf16();
        // the operators @, # and :. become SINGLE/ANCHORARRAY/_TRO_*: internal, not typed by name
        for op in ["SINGLE", "ANCHORARRAY", "_TRO_TRAILING", "_TRO_LEADING", "_TRO_ALL"] {
            if nm == u16s(op) {
                return;
            }
        }
        if syntax.is_reserved(&nm) {
            bad = Some(m.reserved);
            return;
        }
        let nargs = ast.kids(args).len();
        match syntax.arity(&nm, nargs) {
            Arity::Ok => {}
            Arity::Few => bad = Some(m.few),
            Arity::Many => bad = Some(m.many),
        }
    });
    if let Some(message) = bad {
        return Entry::Invalid { message };
    }
    let mut text: Vec<u16> = if canon.first() == Some(&61) { canon[1..].to_vec() } else { canon.clone() };
    // @ and # (screen form) → _xlfn.SINGLE/_xlfn.ANCHORARRAY: the file takes only the function form
    if has_screen_operator(&text) {
        text = print_formula(&ast, root, &PrintOptions { row: ctx.row, col: ctx.col, r1c1: false, ui: false }).to_utf16_vec();
    }
    Entry::Formula { text, ast: Box::new(ast), root }
}

trait ToUtf16Vec {
    fn to_utf16_vec(&self) -> Vec<u16>;
}

impl ToUtf16Vec for uo_calc::strings::JsString {
    fn to_utf16_vec(&self) -> Vec<u16> {
        self.as_js().to_utf16()
    }
}

/// A constant operand (number, text, boolean, error, array), even inside parentheses.
fn is_constant(ast: &Ast, mut n: NodeId) -> bool {
    while let Node::Paren(e) = *ast.node(n) {
        n = e;
    }
    matches!(*ast.node(n), Node::Num { .. } | Node::Str(_) | Node::Bool(_) | Node::Err(_) | Node::Arr { .. })
}

/// Outside texts and quoted names, a number with `.` when the region's decimal is another.
fn bad_decimal(text: &[u16], decimal: &[u16]) -> bool {
    if decimal == [46] {
        return false;
    }
    let id = |u: u16| -> bool {
        u == 95
            || u == 46
            || u == 92
            || u == 36
            || is_digit(u)
            || uo_text::props::is_letter(u32::from(u))
            || uo_text::props::is_number(u32::from(u))
    };
    let n = text.len();
    let mut i = 1;
    while i < n {
        let ch = text[i];
        if ch == 34 || ch == 39 {
            let q = ch;
            i += 1;
            while i < n {
                if text[i] == q {
                    if text.get(i + 1) == Some(&q) {
                        i += 1;
                    } else {
                        break;
                    }
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        if ch == 91 {
            if let Some(j) = text.iter().skip(i).position(|&u| u == 93) {
                i += j;
            }
            i += 1;
            continue;
        }
        if is_digit(ch) && !id(text[i - 1]) {
            let mut j = i;
            while j < n && is_digit(text[j]) {
                j += 1;
            }
            if text.get(j) == Some(&46) && text.get(j + 1).is_some_and(|&u| is_digit(u)) {
                return true;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    false
}

/// Completes the final `)` as Excel does (measured): only when the formula is ONE function call right after the
/// `=` whose parenthesis is the only one open at the end.
#[must_use]
pub fn auto_close(local: &[u16]) -> Option<Vec<u16>> {
    // /^=\s*[\p{L}_\\][\p{L}\p{N}_.\\]*\(/u
    let n = local.len();
    if local.first() != Some(&61) {
        return None;
    }
    let mut i = 1;
    while i < n && uo_jsnum::text::is_js_space_unit(local[i]) {
        i += 1;
    }
    let (cp, w) = uo_text::utf16::decode_at(local, i)?;
    if !is_ident_start(cp) {
        return None;
    }
    i += w;
    while let Some((c, w2)) = uo_text::utf16::decode_at(local, i) {
        if !(is_ident_start(c) || uo_text::props::is_number(c) || c == 46) {
            break;
        }
        i += w2;
    }
    if local.get(i) != Some(&40) {
        return None;
    }
    let start = i; // index of the `(` (m[0].length - 1)
    let mut depth = 0i32;
    let mut k = start;
    while k < n {
        let ch = local[k];
        if ch == 34 || ch == 39 {
            let q = ch;
            let mut closed = false;
            k += 1;
            while k < n {
                if local[k] == q {
                    if local.get(k + 1) == Some(&q) {
                        k += 1;
                    } else {
                        closed = true;
                        break;
                    }
                }
                k += 1;
            }
            if !closed {
                return None;
            }
            k += 1;
            continue;
        }
        if ch == 40 {
            depth += 1;
        } else if ch == 41 {
            depth -= 1;
            if depth == 0 {
                return None;
            }
        }
        k += 1;
    }
    if depth == 1 {
        let mut out = local.to_vec();
        out.push(41);
        Some(out)
    } else {
        None
    }
}

fn has_screen_operator(text: &[u16]) -> bool {
    const ERRS: [&str; 15] = [
        "NULL!",
        "DIV/0!",
        "VALUE!",
        "REF!",
        "NAME?",
        "NUM!",
        "N/A",
        "GETTING_DATA",
        "SPILL!",
        "CALC!",
        "FIELD!",
        "BLOCKED!",
        "CONNECT!",
        "UNKNOWN!",
        "BUSY!",
    ];
    let n = text.len();
    let mut i = 0;
    let mut q = 0u16;
    while i < n {
        let ch = text[i];
        if q != 0 {
            if ch == q {
                if text.get(i + 1) == Some(&q) {
                    i += 1;
                } else {
                    q = 0;
                }
            }
            i += 1;
            continue;
        }
        if ch == 34 || ch == 39 {
            q = ch;
            i += 1;
            continue;
        }
        if ch == 91
            && let Some(j) = text.iter().skip(i + 1).position(|&u| u == 93)
        {
            i += j + 2;
            continue;
        }
        if ch == 64 || ch == 35 {
            // the `#` of an error (#REF!, #N/A…) is not the spill operator
            if ch == 35 {
                let rest = &text[i + 1..];
                if ERRS.iter().any(|e| rest.len() >= e.len() && ci_eq(&rest[..e.len()], e))
                    || ci_eq(rest.get(..6).unwrap_or(&[]), "PYTHON") && rest.get(6) == Some(&33)
                {
                    i += 1;
                    continue;
                }
            }
            return true;
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------- edit-text.ts

/// `numberEditText`: a number with up to 15 significant digits, as Excel's edit text shows it.
#[must_use]
pub fn number_edit_text(v: f64, decimal: &str) -> String {
    if !v.is_finite() {
        return uo_jsnum::format::to_js_string(v);
    }
    if v == 0.0 {
        return "0".to_owned();
    }
    let neg = v < 0.0;
    let e = uo_jsnum::format::to_exponential(math::abs(v), 14).unwrap_or_default();
    let (m, ex) = e.split_once('e').unwrap_or((e.as_str(), "0"));
    let all: String = m.chars().filter(|&c| c != '.').collect();
    let trimmed = all.trim_end_matches('0');
    let digits: &str = if trimmed.is_empty() { "0" } else { trimmed };
    let exp: i32 = ex.trim_start_matches('+').parse().unwrap_or(0);
    let dlen = i32::try_from(digits.len()).unwrap_or(0);
    let decimals = (dlen - 1 - exp).max(0);
    let s = if exp >= 21 || decimals > 19 {
        let first = &digits[..1];
        let rest = if digits.len() > 1 { format!("{decimal}{}", &digits[1..]) } else { String::new() };
        format!("{first}{rest}E{}{}", if exp < 0 { '-' } else { '+' }, exp.abs())
    } else if exp >= 0 {
        let eu = usize::try_from(exp).unwrap_or(0);
        let (int, frac) = if digits.len() > eu + 1 {
            (digits[..=eu].to_owned(), digits[eu + 1..].to_owned())
        } else {
            (format!("{digits:0<w$}", w = eu + 1), String::new())
        };
        if frac.is_empty() { int } else { format!("{int}{decimal}{frac}") }
    } else {
        format!("0{decimal}{}{digits}", "0".repeat(usize::try_from(-exp - 1).unwrap_or(0)))
    };
    if neg { format!("-{s}") } else { s }
}

fn short_date(serial: f64, loc: &FormulaLocale, date1904: bool) -> String {
    let y = uo_calc::dates::serial_to_ymd(math::floor(serial), date1904);
    let sep = loc.values.date_sep.as_js().to_string_lossy();
    let (yv, mv, dv) = (num_text(y.y), num_text(y.m), num_text(y.d));
    let pad2 = |s: &str| if s.len() < 2 { format!("0{s}") } else { s.to_owned() };
    match loc.values.date_order {
        DateOrder::Mdy => format!("{mv}{sep}{dv}{sep}{yv}"),
        DateOrder::Ymd => format!("{yv}-{}-{}", pad2(&mv), pad2(&dv)),
        DateOrder::Dmy => format!("{}{sep}{}{sep}{yv}", pad2(&dv), pad2(&mv)),
    }
}

fn num_text(x: f64) -> String {
    uo_jsnum::format::to_js_string(x)
}

fn time_of_day(serial: f64) -> String {
    let h = uo_calc::dates::serial_to_hms(serial - math::floor(serial));
    let s = if math::round(h.s) >= 60.0 { 59.0 } else { math::round(h.s) };
    let p = |x: f64| {
        let t = num_text(x);
        if t.len() < 2 { format!("0{t}") } else { t }
    };
    format!("{}:{}:{}", p(h.h), p(h.mi), p(s))
}

/// A constant to edit (`Scalar` without the pool): the value of a cell.
#[derive(Clone, PartialEq, Debug)]
pub enum EditValue<'a> {
    /// Empty cell.
    Empty,
    /// Number.
    Number(f64),
    /// Text.
    Text(&'a [u16]),
    /// Boolean.
    Bool(bool),
    /// Error.
    Error(ErrorCode),
}

/// `editText`: the edit text of a value in a cell whose format has category `cat`.
#[must_use]
pub fn edit_text(v: &EditValue<'_>, cat: FmtCategory, loc: &FormulaLocale, date1904: bool, quote_prefix: bool) -> Vec<u16> {
    match v {
        EditValue::Empty => Vec::new(),
        EditValue::Bool(b) => u16s(if loc.lang == FormulaLang::PtBr {
            if *b { "VERDADEIRO" } else { "FALSO" }
        } else if *b {
            "TRUE"
        } else {
            "FALSE"
        }),
        EditValue::Error(c) => u16s(error_text(*c, loc.lang)),
        EditValue::Text(t) => {
            if quote_prefix {
                let mut o = vec![39];
                o.extend_from_slice(t);
                o
            } else {
                t.to_vec()
            }
        }
        EditValue::Number(v) => {
            let v = *v;
            if cat == FmtCategory::Percent {
                return u16s(&format!("{}%", number_edit_text(v * 100.0, &loc.decimal)));
            }
            if matches!(cat, FmtCategory::Date | FmtCategory::Time) && (0.0..2_958_466.0).contains(&v) {
                if cat == FmtCategory::Time && v < 1.0 {
                    return u16s(&time_of_day(v));
                }
                let frac = v - math::floor(v);
                let mut s = short_date(v, loc, date1904);
                if frac > 1e-9 {
                    s.push(' ');
                    s.push_str(&time_of_day(v));
                }
                return u16s(&s);
            }
            u16s(&number_edit_text(v, &loc.decimal))
        }
    }
}

/// `localizeError(code, lang)` (the typable ones plus the new ones, as the calc's i18n tables).
#[must_use]
pub fn error_text(c: ErrorCode, lang: FormulaLang) -> &'static str {
    let (en, pt) = match c {
        ErrorCode::Null => ("#NULL!", "#NULO!"),
        ErrorCode::Div0 => ("#DIV/0!", "#DIV/0!"),
        ErrorCode::Value => ("#VALUE!", "#VALOR!"),
        ErrorCode::Ref => ("#REF!", "#REF!"),
        ErrorCode::Name => ("#NAME?", "#NOME?"),
        ErrorCode::Num => ("#NUM!", "#NÚM!"),
        ErrorCode::NA => ("#N/A", "#N/D"),
        ErrorCode::GettingData => ("#GETTING_DATA", "#OBTENDO_DADOS"),
        ErrorCode::Spill => ("#SPILL!", "#DESPEJAR!"),
        ErrorCode::Connect => ("#CONNECT!", "#CONECTAR!"),
        ErrorCode::Blocked => ("#BLOCKED!", "#BLOQUEADO!"),
        ErrorCode::Unknown => ("#UNKNOWN!", "#DESCONHECIDO!"),
        ErrorCode::Field => ("#FIELD!", "#CAMPO!"),
        ErrorCode::Calc => ("#CALC!", "#CALC!"),
        ErrorCode::Busy => ("#BUSY!", "#OCUPADO!"),
        ErrorCode::Python => ("#PYTHON!", "#PYTHON!"),
    };
    if lang == FormulaLang::PtBr { pt } else { en }
}

/// The catalogue prefix of a function name in the registry (`catalogEntry(name).prefix`), for hosts that build
/// file formulas.
#[must_use]
pub fn file_prefix(name: &[u16]) -> Option<&'static str> {
    let stripped = uo_calc::fnreg::strip_file_prefix(JsStr::Utf16(name));
    let ix = catalog_entry(JsStr::Utf16(&stripped))?;
    let p: CatPrefix = ix.entry().catalog;
    p.text()
}
