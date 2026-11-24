//! The Excel function catalogue (`i18n/catalog.ts`, frontier A5): the measured list of worksheet functions of
//! Excel 365 (internal number, file prefix, accepted argument counts), the names the Excel table refuses in a cell
//! (XLM macro and internal functions) and the names without effect in this build. The data is generated
//! ([`crate::i18n::tables`], `js/gen-i18n-tables.mjs`).
//!
//! The autocomplete list ([`function_names`]) is returned in catalogue order: the shell sorts it with the same
//! `Intl.Collator(lang.id)` call as the TS (`translate.ts:684-692`; plan §16 item 11) — no collator crosses the
//! boundary.

use crate::i18n::tables::{CATALOG, CATALOG_BY_NAME, OPERATOR_FUNCTIONS, RESERVED, UNAVAILABLE};
use crate::i18n::{FormulaLanguage, resolve_language};
use crate::strings::JsStr;

pub use crate::i18n::CatalogEntry;
pub use crate::i18n::tables::EXCEL_BUILD;

/// The upper-cased name without the file prefixes `_xlfn.`, `_xlws.`, `_xlfn._xlws.` (`stripFilePrefix`), written
/// into `out` (cleared first) as UTF-8 — every catalogue name is ASCII, so a name with another character is
/// written lossily (it can match nothing).
pub fn strip_file_prefix_into(name: JsStr<'_>, out: &mut String) {
    upper_utf8_into(name, out);
    let cut = out.len() - strip_upper_prefix(out).len();
    out.drain(..cut);
}

/// The `replace(/^(_XLFN\.)?(_XLWS\.)?/, '')` of an already upper-cased name.
#[must_use]
pub fn strip_upper_prefix(s: &str) -> &str {
    let s = s.strip_prefix("_XLFN.").unwrap_or(s);
    s.strip_prefix("_XLWS.").unwrap_or(s)
}

/// `s.toUpperCase()` (full Unicode mapping: `ſ` → `S`, `ß` → `SS`) as UTF-8 into `out` (cleared first); a lone
/// surrogate becomes U+FFFD (it can match no table key). No allocation once `out` has grown.
pub fn upper_utf8_into(s: JsStr<'_>, out: &mut String) {
    out.clear();
    let mut it = s.units().peekable();
    while let Some(u) = it.next() {
        if u < 0x80 {
            out.push((u as u8).to_ascii_uppercase() as char);
            continue;
        }
        let cp = if (0xD800..0xDC00).contains(&u) {
            match it.peek() {
                Some(&lo) if (0xDC00..0xE000).contains(&lo) => {
                    it.next();
                    0x10000 + ((u32::from(u) - 0xD800) << 10) + (u32::from(lo) - 0xDC00)
                }
                _ => 0xFFFD,
            }
        } else if (0xDC00..0xE000).contains(&u) {
            0xFFFD
        } else {
            u32::from(u)
        };
        match uo_text::case::upper_mapping(cp) {
            uo_text::case::CaseMapping::Same => out.push(char::from_u32(cp).unwrap_or('\u{FFFD}')),
            uo_text::case::CaseMapping::One(m) => out.push(char::from_u32(m).unwrap_or('\u{FFFD}')),
            uo_text::case::CaseMapping::Many(units) => {
                for c in char::decode_utf16(units.iter().copied()) {
                    out.push(c.unwrap_or('\u{FFFD}'));
                }
            }
        }
    }
}

/// The catalogue entry of an exact canonical name (upper case, no prefix).
#[must_use]
pub fn entry_exact(name: &str) -> Option<&'static CatalogEntry> {
    CATALOG_BY_NAME.binary_search_by(|&i| CATALOG[usize::from(i)].name.cmp(name)).ok().map(|k| &CATALOG[usize::from(CATALOG_BY_NAME[k])])
}

/// `catalogEntry(name)`: a worksheet function known to Excel (with or without a file prefix, any case).
#[must_use]
pub fn catalog_entry(name: JsStr<'_>) -> Option<&'static CatalogEntry> {
    let mut s = String::new();
    strip_file_prefix_into(name, &mut s);
    entry_exact(&s)
}

/// `catalog()`: every worksheet function, in the order of the Excel internal number.
#[must_use]
pub fn catalog() -> &'static [CatalogEntry] {
    &CATALOG
}

/// How Excel treats a function name in a cell formula (`functionKind`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FunctionKind {
    /// A worksheet function.
    Sheet,
    /// A name of the Excel table refused in a cell (XLM macro or internal function): typing it is a formula error.
    Reserved,
    /// Unknown (UDF, a name not released in this build): `#NAME?`.
    Unknown,
}

/// `functionKind(name)`.
#[must_use]
pub fn function_kind(name: JsStr<'_>) -> FunctionKind {
    let mut s = String::new();
    strip_file_prefix_into(name, &mut s);
    if entry_exact(&s).is_some() {
        FunctionKind::Sheet
    } else if RESERVED.binary_search(&s.as_str()).is_ok() {
        FunctionKind::Reserved
    } else {
        FunctionKind::Unknown
    }
}

/// `isUnavailableFunction(name)`: in the Excel table but without effect in this build (`#NAME?`).
#[must_use]
pub fn is_unavailable_function(name: JsStr<'_>) -> bool {
    let mut s = String::new();
    strip_file_prefix_into(name, &mut s);
    UNAVAILABLE.binary_search(&s.as_str()).is_ok()
}

/// `acceptsArgCount(e, n)`: the Excel parser accepts the function with `n` arguments (`n` a JS number).
#[must_use]
pub fn accepts_arg_count(e: &CatalogEntry, n: f64) -> bool {
    if n < f64::from(e.min_args) || n > f64::from(e.max_args) {
        return false;
    }
    if n <= 12.0 && !e.small_counts.is_empty() {
        return e.small_counts.iter().any(|&k| f64::from(k) == n);
    }
    if e.arg_step == 1 {
        return true;
    }
    // `(maxArgs - n) % 2 === 0` (JS `%` keeps the sign; a non-integer n never passes)
    (f64::from(e.max_args) - n) % 2.0 == 0.0
}

/// `withFilePrefix(name)`: the name with the prefix the OOXML serializer writes (unknown names unchanged).
#[must_use]
pub fn with_file_prefix(name: JsStr<'_>) -> crate::strings::JsString {
    match catalog_entry(name) {
        Some(e) => crate::strings::JsString::from_str_js(&format!("{}{}", e.prefix, e.name)),
        None => name.to_owned_js(),
    }
}

/// `OPERATOR_FUNCTIONS[name]`: the screen operator of a function shown as an operator (`SINGLE` → `@`).
#[must_use]
pub fn operator_of(name: &str) -> Option<&'static str> {
    OPERATOR_FUNCTIONS.binary_search_by(|(k, _)| (*k).cmp(name)).ok().map(|i| OPERATOR_FUNCTIONS[i].1)
}

/// One autocomplete entry (`FunctionName`): the name in the language, the canonical name, the measured arity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FunctionName {
    /// Name in the language.
    pub name: &'static str,
    /// Canonical name.
    pub canonical: &'static str,
    /// Minimum arguments.
    pub min_args: u16,
    /// Maximum arguments.
    pub max_args: u16,
}

/// `functionNames(lang)` WITHOUT the collator sort: the worksheet functions (operators excluded) in catalogue
/// order; the shell sorts them with `Intl.Collator(lang.id)` (record "A5", plan §16 item 11).
#[must_use]
pub fn function_names(lang: &FormulaLanguage) -> Vec<FunctionName> {
    CATALOG
        .iter()
        .filter(|e| e.operator.is_none())
        .map(|e| FunctionName { name: lang.local_function(e.name), canonical: e.name, min_args: e.min_args, max_args: e.max_args })
        .collect()
}

/// [`function_names`] for a language id (`resolveLanguage`: unknown → English).
#[must_use]
pub fn function_names_for(id: &str) -> Vec<FunctionName> {
    function_names(resolve_language(Some(id)))
}
