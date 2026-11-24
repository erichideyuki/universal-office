//! The formula-language layer (`i18n/*`, frontier A5): the Excel language tables (en-US canonical, pt-BR measured
//! on the oracle), the list of the Office interface languages and the text translator canonical ↔ local
//! ([`Translator`]: `toLocal`/`fromLocal`). The engine only sees the canonical form.
//!
//! The value conventions of a language (`valueLocaleFor`) are [`crate::locale::ValueLocale::pt_br`]/`en_us` for the
//! measured ids ([`value_locale_for`]); any other id is derived by the shell from `Intl` (decision C1: Rust never
//! calls `Intl`). The `Intl` decimal of a language outside the tables (`regionalDecimal`) is the host hook
//! [`crate::locale::HostHooks::regional_decimal`], passed in [`SyntaxOptions::regional_decimal`].

// generated (`js/gen-i18n-tables.mjs`, `--check` keeps it current)
#[rustfmt::skip]
pub mod tables;
mod translate;

pub use translate::{LangSpec, NameSet, ResolvedSyntax, SyntaxOptions, Translator, resolve_syntax};

use crate::locale::ValueLocale;
use crate::strings::JsStr;
use crate::value::ErrorCode;

/// One catalogue entry (`CatalogEntry`, `i18n/catalog.ts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogEntry {
    /// Canonical name (English, no prefix).
    pub name: &'static str,
    /// Excel internal number (0–367 = BIFF8 Ftab).
    pub id: u16,
    /// The prefix Excel writes in the file: `""`, `_xlfn.` or `_xlfn._xlws.`.
    pub prefix: &'static str,
    /// Minimum arguments.
    pub min_args: u16,
    /// Maximum arguments the Excel parser accepts.
    pub max_args: u16,
    /// 2 = the valid counts step by two (criterion/value pairs).
    pub arg_step: u8,
    /// Accepted counts up to 12 when they do not follow (min, step); empty = none.
    pub small_counts: &'static [u8],
    /// Shown as an operator on screen (`SINGLE` ↔ `@`).
    pub operator: Option<&'static str>,
}

/// A structured-reference keyword.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kw {
    /// `#All`.
    All,
    /// `#Data`.
    Data,
    /// `#Headers`.
    Headers,
    /// `#Totals`.
    Totals,
    /// `#This Row`.
    ThisRow,
}

impl Kw {
    /// Index in [`FormulaLanguage::kw_out`].
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// R1C1 letters and relative brackets of a language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct R1c1Syntax {
    /// Row letter.
    pub row: &'static str,
    /// Column letter.
    pub col: &'static str,
    /// Opening bracket of a relative offset.
    pub open: Option<&'static str>,
    /// Closing bracket.
    pub close: Option<&'static str>,
}

/// The structured-reference keywords a language declares (absent = canonical).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableKeywordsOpt {
    /// `#All`.
    pub all: Option<&'static str>,
    /// `#Data`.
    pub data: Option<&'static str>,
    /// `#Headers`.
    pub headers: Option<&'static str>,
    /// `#Totals`.
    pub totals: Option<&'static str>,
    /// `#This Row`.
    pub this_row: Option<&'static str>,
}

/// Array-constant separators of a language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArraySeparators {
    /// Column separator.
    pub col: &'static str,
    /// Row separator.
    pub row: &'static str,
    /// Alternative used when the column separator is the decimal.
    pub alt: Option<&'static str>,
}

/// Format-code letters and the "General" name of a language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatCodesOpt {
    /// Year.
    pub year: Option<&'static str>,
    /// Month.
    pub month: Option<&'static str>,
    /// Day.
    pub day: Option<&'static str>,
    /// Hour.
    pub hour: Option<&'static str>,
    /// Minute.
    pub minute: Option<&'static str>,
    /// Second.
    pub second: Option<&'static str>,
    /// "General".
    pub general: Option<&'static str>,
}

/// Typical regional separators of the language's users.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Regional {
    /// Decimal separator.
    pub decimal: &'static str,
    /// List separator.
    pub list: &'static str,
}

/// The table of one Excel language (`FormulaLanguage`, `i18n/types.ts`) plus the lookup maps `translate.ts`
/// `langData` builds (generated with the TS semantics).
#[derive(Debug, PartialEq, Eq)]
pub struct FormulaLanguage {
    /// BCP 47 id.
    pub id: &'static str,
    /// Windows/Office LCID.
    pub lcid: Option<u32>,
    /// Name of the language in itself.
    pub name: &'static str,
    /// Local name of each translated function (canonical without prefix → local), sorted by canonical name.
    pub functions: &'static [(&'static str, &'static str)],
    /// Local names of the errors that change.
    pub errors: &'static [(ErrorCode, &'static str)],
    /// TRUE/FALSE.
    pub bools: Option<[&'static str; 2]>,
    /// R1C1 letters.
    pub r1c1: Option<R1c1Syntax>,
    /// Structured-reference keywords that change.
    pub table: TableKeywordsOpt,
    /// Array-constant separators.
    pub array: Option<ArraySeparators>,
    /// Format codes.
    pub format: Option<FormatCodesOpt>,
    /// Regional defaults.
    pub regional: Option<Regional>,
    /// Where the data come from.
    pub source: &'static str,
    /// Folded local name (exact and without diacritics) → canonical, sorted by key (code-point order).
    pub fn_in: &'static [(&'static str, &'static str)],
    /// Folded local error name → code, longest first (the TS `errIn` order).
    pub err_in: &'static [(&'static str, ErrorCode)],
    /// Folded keyword → keyword.
    pub kw_in: &'static [(&'static str, Kw)],
    /// Keyword texts in [`Kw`] order (canonical where the language has none).
    pub kw_out: [&'static str; 5],
}

impl FormulaLanguage {
    /// `fnOut(canon)`: the local name of a canonical function name (itself when not translated).
    #[must_use]
    pub fn local_function<'a>(&self, canon: &'a str) -> &'a str
    where
        'static: 'a,
    {
        match self.functions.binary_search_by(|(k, _)| (*k).cmp(canon)) {
            Ok(i) => self.functions[i].1,
            Err(_) => canon,
        }
    }

    /// `errOut(code)`: the local error text.
    #[must_use]
    pub fn error_text(&self, code: ErrorCode) -> &'static str {
        self.errors.iter().find(|(c, _)| *c == code).map_or_else(|| crate::cell::error_text(code), |(_, t)| *t)
    }

    /// TRUE/FALSE texts (`lang.bools ?? ['TRUE', 'FALSE']`).
    #[must_use]
    pub fn bool_texts(&self) -> [&'static str; 2] {
        self.bools.unwrap_or(["TRUE", "FALSE"])
    }

    /// R1C1 row letter (`R` by default).
    #[must_use]
    pub fn row_letter(&self) -> &'static str {
        self.r1c1.map_or("R", |r| r.row)
    }

    /// R1C1 column letter (`C` by default).
    #[must_use]
    pub fn col_letter(&self) -> &'static str {
        self.r1c1.map_or("C", |r| r.col)
    }

    /// The folded-name lookup (`fnIn.get(key)`).
    #[must_use]
    pub fn fn_in_get(&self, key: &str) -> Option<&'static str> {
        self.fn_in.binary_search_by(|(k, _)| (*k).cmp(key)).ok().map(|i| self.fn_in[i].1)
    }
}

/// Function names of the Excel of a language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionNames {
    /// Translated.
    Localized,
    /// English names.
    English,
    /// Not known.
    Unknown,
}

/// An Office interface language (`ExcelLanguageInfo`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExcelLanguageInfo {
    /// BCP 47 id.
    pub id: &'static str,
    /// LCID.
    pub lcid: u32,
    /// Name in itself.
    pub name: &'static str,
    /// Function names of its Excel.
    pub function_names: FunctionNames,
    /// The variant used when only the language is asked (`de` → `de-DE`).
    pub main: bool,
    /// Measured on this machine's oracle.
    pub measured: bool,
}

/// The canonical language (`EN_US_LANG`).
pub static EN_US: &FormulaLanguage = &tables::EN_US_LANG;
/// pt-BR (`PT_BR_LANG`).
pub static PT_BR: &FormulaLanguage = &tables::PT_BR_LANG;

/// `languages()`: the registered tables, in registration order.
#[must_use]
pub fn languages() -> [&'static FormulaLanguage; 2] {
    [EN_US, PT_BR]
}

/// `EXCEL_LANGUAGES`.
#[must_use]
pub fn excel_languages() -> &'static [ExcelLanguageInfo] {
    &tables::EXCEL_LANGUAGES
}

/// `getLanguage(id)`: exact id (case-insensitive) → the main variant of the same language (`de-AT` → `de-DE`) →
/// `None`. pt-BR and pt-PT are different languages in Excel (`pt` → pt-PT, which has no table).
#[must_use]
pub fn get_language(id: &str) -> Option<&'static FormulaLanguage> {
    if !id.is_ascii() {
        return get_language_slow(id);
    }
    // ASCII ids (every real one): case-insensitive comparisons, no allocation
    if let Some(l) = languages().into_iter().find(|l| l.id.eq_ignore_ascii_case(id)) {
        return Some(l);
    }
    let base = id.split('-').next().unwrap_or("");
    let main = tables::EXCEL_LANGUAGES.iter().find(|l| l.main && l.id.split('-').next().is_some_and(|b| b.eq_ignore_ascii_case(base)))?;
    languages().into_iter().find(|l| l.id.eq_ignore_ascii_case(main.id))
}

/// [`get_language`] of a non-ASCII id (full Unicode `toLowerCase`).
fn get_language_slow(id: &str) -> Option<&'static FormulaLanguage> {
    let k = js_lower(id);
    if let Some(l) = languages().into_iter().find(|l| js_lower(l.id) == k) {
        return Some(l);
    }
    let base = k.split('-').next().unwrap_or("");
    let main = tables::EXCEL_LANGUAGES.iter().find(|l| js_lower(l.id).split('-').next() == Some(base) && l.main)?;
    let mk = js_lower(main.id);
    languages().into_iter().find(|l| js_lower(l.id) == mk)
}

/// `toLowerCase()` of an id (full Unicode mapping, as V8).
fn js_lower(s: &str) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    String::from_utf16_lossy(&uo_text::case::to_lowercase(&u))
}

/// `resolveLanguage(lang)`: the table of the id (unknown → English, as an Excel without translation).
#[must_use]
pub fn resolve_language(id: Option<&str>) -> &'static FormulaLanguage {
    id.and_then(get_language).unwrap_or(EN_US)
}

/// `valueLocaleFor(id)` for the measured ids (`pt-br`, `en-us`, any case): the measured tables. Any other id is the
/// shell's `deriveValueLocale` (decision C1) — `None` here.
#[must_use]
pub fn value_locale_for(id: &str) -> Option<ValueLocale> {
    match js_lower(id).as_str() {
        "pt-br" => Some(ValueLocale::pt_br()),
        "en-us" => Some(ValueLocale::en_us()),
        _ => None,
    }
}

/// `localFunctionName(canonical, lang)`: the name of a function in the language (canonical with or without prefix).
#[must_use]
pub fn local_function_name(canonical: JsStr<'_>, lang: &FormulaLanguage) -> crate::strings::JsString {
    let mut k = String::new();
    crate::catalogue::strip_file_prefix_into(canonical, &mut k);
    match lang.functions.binary_search_by(|(c, _)| (*c).cmp(k.as_str())) {
        Ok(i) => crate::strings::JsString::from_str_js(lang.functions[i].1),
        // `l.functions[k] ?? k`: the upper-cased, prefix-less name (lossless: rebuild from UTF-16)
        Err(_) => {
            let u: Vec<u16> = canonical.units().collect();
            let up = uo_text::case::to_uppercase(&u);
            let mut cut = 0;
            let starts = |s: &[u16], p: &str| s.len() >= p.len() && s.iter().zip(p.encode_utf16()).all(|(a, b)| *a == b);
            if starts(&up, "_XLFN.") {
                cut = 6;
            }
            if starts(&up[cut..], "_XLWS.") {
                cut += 6;
            }
            crate::strings::JsString::from_utf16(&up[cut..])
        }
    }
}

/// `canonicalFunctionName(local, lang)`: the canonical name of a function typed in the language (`None` = unknown
/// in that language). Exact folded name, or the name without diacritics (`MAXIMO` → `MAX`).
#[must_use]
pub fn canonical_function_name(local: JsStr<'_>, lang: &FormulaLanguage) -> Option<&'static str> {
    let mut t = Translator::new();
    let u: Vec<u16> = local.units().collect();
    t.fn_lookup(lang, &u)
}

/// `localizeError(code, lang)`: the error in the language (`#N/A` → `#N/D`).
#[must_use]
pub fn localize_error(code: ErrorCode, lang: &FormulaLanguage) -> &'static str {
    lang.error_text(code)
}

/// `canonicalError(text, lang)`: an error typed in the language (trimmed, any case) → its code.
#[must_use]
pub fn canonical_error(text: &[u16], lang: &FormulaLanguage) -> Option<ErrorCode> {
    let t = crate::jstext::js_trim(text);
    let mut f = String::new();
    crate::catalogue::upper_utf8_into(JsStr::Utf16(t), &mut f);
    lang.err_in.iter().find(|(name, _)| *name == f).map(|(_, c)| *c)
}

/// `localizeBool(b, lang)`.
#[must_use]
pub fn localize_bool(b: bool, lang: &FormulaLanguage) -> &'static str {
    lang.bool_texts()[usize::from(!b)]
}
