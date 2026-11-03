//! Frontier R2 (D1a reader, record "Milestone 1 — D1a foundation"): `style-names-es.ts`:
//! `tableStyleNameEs` with its `TABLE_FIXED` and `TABLE_FAMILY` rules; the name table
//! (`BUILTIN_STYLE_NAMES_ES`) is `data::style_names_es`.
//!
//! Regex sites: `/^(.*?)(?: Accent (\d))?$/` and the 29 anchored `TABLE_FAMILY` patterns (each is
//! `^PREFIX(\d)SUFFIX$` or a fixed name), written as scans; rows in `js/regex-sites/r2.mjs`.
//! Two named differences from the TS, which returns garbage or throws there (declared in
//! `uo-docx-d1a/R2.md`, item 9): a name with a line terminator makes the TS throw a `TypeError`
//! (`exec` is `null`), here it is `None`; and `TABLE_FIXED` is a plain object, so a base named
//! like an `Object.prototype` member (`constructor`, `toString`, …) finds a function there, here
//! it is `None`.

use crate::data::style_names_es::BUILTIN_STYLE_NAMES_ES;

/// `TABLE_FIXED`.
const TABLE_FIXED: [(&str, &str); 7] = [
    ("Normal Table", "Tabla normal"),
    ("Table Grid", "Tabla con cuadrícula"),
    ("Grid Table Light", "Tabla con cuadrícula clara"),
    ("Table Theme", "Tabla con tema"),
    ("Table Contemporary", "Tabla contemporánea"),
    ("Table Elegant", "Tabla elegante"),
    ("Table Professional", "Tabla profesional"),
];

/// A `TABLE_FAMILY` rule: `^PRE(\d)POST$` (or the whole name when `digit` is false) and its
/// replacement `ES_PRE$1ES_POST`.
struct Family {
    pre: &'static str,
    digit: bool,
    post: &'static str,
    es_pre: &'static str,
    es_post: &'static str,
}

const fn with_digit(pre: &'static str, post: &'static str, es_pre: &'static str, es_post: &'static str) -> Family {
    Family { pre, digit: true, post, es_pre, es_post }
}

const fn fixed(name: &'static str, es: &'static str) -> Family {
    Family { pre: name, digit: false, post: "", es_pre: es, es_post: "" }
}

/// `TABLE_FAMILY`, in the TS order (the first rule that matches wins).
const TABLE_FAMILY: [Family; 29] = [
    with_digit("Grid Table ", "", "Tabla de cuadrícula ", ""),
    with_digit("Grid Table ", " Light", "Tabla de cuadrícula ", " clara"),
    with_digit("Grid Table ", " Dark", "Tabla de cuadrícula ", " oscura"),
    with_digit("Grid Table ", " Colorful", "Tabla de cuadrícula ", " con colores"),
    with_digit("List Table ", "", "Tabla de lista ", ""),
    with_digit("List Table ", " Light", "Tabla de lista ", " clara"),
    with_digit("List Table ", " Dark", "Tabla de lista ", " oscura"),
    with_digit("List Table ", " Colorful", "Tabla de lista ", " con colores"),
    with_digit("Plain Table ", "", "Tabla normal ", ""),
    fixed("Light Shading", "Sombreado claro"),
    with_digit("Medium Shading ", "", "Sombreado medio ", ""),
    fixed("Dark Shading", "Sombreado oscuro"),
    fixed("Colorful Shading", "Sombreado vistoso"),
    fixed("Light List", "Lista clara"),
    with_digit("Medium List ", "", "Lista media ", ""),
    fixed("Dark List", "Lista oscura"),
    fixed("Colorful List", "Lista vistosa"),
    fixed("Light Grid", "Cuadrícula clara"),
    with_digit("Medium Grid ", "", "Cuadrícula media ", ""),
    fixed("Colorful Grid", "Cuadrícula vistosa"),
    with_digit("Table Classic ", "", "Tabla clásica ", ""),
    with_digit("Table Colorful ", "", "Tabla con colores ", ""),
    with_digit("Table 3D effects ", "", "Tabla con efectos 3D ", ""),
    with_digit("Table Grid ", "", "Tabla con cuadrícula ", ""),
    with_digit("Table Web ", "", "Tabla web ", ""),
    with_digit("Table Columns ", "", "Tabla con columnas ", ""),
    with_digit("Table List ", "", "Tabla con lista ", ""),
    with_digit("Table Simple ", "", "Tabla sencilla ", ""),
    with_digit("Table Subtle ", "", "Tabla sutil ", ""),
];

/// `re.test(base)` and `base.replace(re, out)` of a rule: the replacement when the whole `base`
/// is matched.
fn family_name(rule: &Family, base: &str) -> Option<String> {
    let rest = base.strip_prefix(rule.pre)?;
    if !rule.digit {
        return rest.is_empty().then(|| rule.es_pre.to_string());
    }
    let mut chars = rest.chars();
    let d = chars.next().filter(char::is_ascii_digit)?;
    (chars.as_str() == rule.post).then(|| format!("{}{d}{}", rule.es_pre, rule.es_post))
}

/// The number of `TABLE_FAMILY` rules.
pub const FAMILY_RULES: usize = TABLE_FAMILY.len();

/// `base.replace(TABLE_FAMILY[rule][0], TABLE_FAMILY[rule][1])`: the replacement when the rule
/// matches the whole `base`, `base` itself otherwise (what `String.prototype.replace` returns).
#[must_use]
pub fn family_replace(rule: usize, base: &str) -> String {
    TABLE_FAMILY.get(rule).and_then(|r| family_name(r, base)).unwrap_or_else(|| base.to_string())
}

/// `.` of a regex without the `s` flag does not match these.
fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `/^(.*?)(?: Accent (\d))?$/`: the base and the accent digit. The lazy base ends where the
/// optional ` Accent d` reaches the end of the name, so only a trailing ` Accent d` is split off;
/// `None` when the name holds a line terminator (the TS `exec` is `null`).
#[must_use]
pub fn split_accent(name: &str) -> Option<(&str, Option<char>)> {
    if name.chars().any(is_line_terminator) {
        return None;
    }
    if let Some(at) = name.len().checked_sub(9)
        && let Some((head, tail)) = name.split_at_checked(at)
        && let Some(d) = tail.strip_prefix(" Accent ").and_then(|t| t.chars().next().filter(char::is_ascii_digit))
    {
        return Some((head, Some(d)));
    }
    Some((name, None))
}

/// `tableStyleNameEs` (style-names-es.ts:75): the Spanish name of a built-in table style by its
/// `w:name` (`Grid Table 4 Accent 1`), `None` when it is not known.
#[must_use]
pub fn table_style_name_es(name: &str) -> Option<String> {
    let (base, accent) = split_accent(name)?;
    let es = match TABLE_FIXED.iter().find(|(k, _)| *k == base) {
        Some((_, v)) => (*v).to_string(),
        None => TABLE_FAMILY.iter().find_map(|r| family_name(r, base))?,
    };
    Some(match accent {
        Some(a) => format!("{es} - Énfasis {a}"),
        None => es,
    })
}

/// `BUILTIN_STYLE_NAMES_ES[key]`: the Spanish display name of a built-in paragraph or character
/// style by its lower-case `w:name`.
#[must_use]
pub fn builtin_style_name_es(key: &str) -> Option<&'static str> {
    BUILTIN_STYLE_NAMES_ES.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}
