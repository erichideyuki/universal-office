//! The number formats of the toolbars (Home › Number in Excel): the codes Excel writes for each item of the list and
//! Increase/Decrease Decimal. Port of `sheet/src/number-formats.ts`. The language is the one of the FORMULAS (pt-BR
//! writes `R$` and `dd/mm/yyyy`, the rest the en-US ones), the same criterion as the entry of cells.

use super::entry::{FormulaLang, currency_hint};

/// An item of the number-format list (`NumberFormatKind`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NumberFormatKind {
    /// General.
    General,
    /// Number.
    Number,
    /// Comma style.
    Comma,
    /// Currency.
    Currency,
    /// Accounting.
    Accounting,
    /// Short date.
    ShortDate,
    /// Long date.
    LongDate,
    /// Time.
    Time,
    /// Percentage with 2 decimals.
    Percent,
    /// Percentage without decimals.
    Percent0,
    /// Fraction.
    Fraction,
    /// Scientific.
    Scientific,
    /// Text.
    Text,
}

/// A fixed internal id or just the code (the pool finds or creates the id).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NumberFormatSpec {
    /// numFmtId when fixed.
    pub id: Option<u32>,
    /// formatCode.
    pub code: String,
}

/// `numberFormatFor(kind, lang)`.
#[must_use]
pub fn number_format_for(kind: NumberFormatKind, lang: FormulaLang) -> NumberFormatSpec {
    let pt = lang == FormulaLang::PtBr;
    let spec = |id: Option<u32>, code: &str| NumberFormatSpec { id, code: code.to_owned() };
    match kind {
        NumberFormatKind::General => spec(Some(0), "General"),
        NumberFormatKind::Number => spec(Some(2), "0.00"),
        // thousands separator (Comma style): the Accounting without a symbol
        NumberFormatKind::Comma => spec(
            None,
            if pt { "_-* #,##0.00_-;\\-* #,##0.00_-;_-* \"-\"??_-;_-@_-" } else { "_(* #,##0.00_);_(* \\(#,##0.00\\);_(* \"-\"??_);_(@_)" },
        ),
        NumberFormatKind::Currency => {
            NumberFormatSpec { id: None, code: currency_hint(if pt { "R$" } else { "$" }, true).code.unwrap_or_default() }
        }
        NumberFormatKind::Accounting => spec(
            None,
            if pt {
                "_-\"R$\"\\ * #,##0.00_-;\\-\"R$\"\\ * #,##0.00_-;_-\"R$\"\\ * \"-\"??_-;_-@_-"
            } else {
                "_(\"$\"* #,##0.00_);_(\"$\"* \\(#,##0.00\\);_(\"$\"* \"-\"??_);_(@_)"
            },
        ),
        NumberFormatKind::ShortDate => spec(Some(14), if pt { "dd/mm/yyyy" } else { "m/d/yyyy" }),
        NumberFormatKind::LongDate => spec(None, if pt { "dddd, d\" de \"mmmm\" de \"yyyy" } else { "dddd, mmmm d, yyyy" }),
        NumberFormatKind::Time => {
            if pt {
                spec(Some(21), "hh:mm:ss")
            } else {
                spec(Some(19), "h:mm:ss AM/PM")
            }
        }
        NumberFormatKind::Percent => spec(Some(10), "0.00%"),
        NumberFormatKind::Percent0 => spec(Some(9), "0%"),
        NumberFormatKind::Fraction => spec(Some(12), "# ?/?"),
        NumberFormatKind::Scientific => spec(Some(11), "0.00E+00"),
        NumberFormatKind::Text => spec(Some(49), "@"),
    }
}

/// Sections of a code (`;` outside quotes, brackets and escapes).
fn sections(code: &[char]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut i = 0;
    while i < code.len() {
        let ch = code[i];
        if ch == '"' || ch == '[' {
            let close = if ch == '"' { '"' } else { ']' };
            let j = code.iter().skip(i + 1).position(|&c| c == close).map(|k| i + 1 + k);
            let end = j.map_or(code.len(), |j| j + 1);
            cur.extend(&code[i..end]);
            i = end;
            continue;
        }
        if matches!(ch, '\\' | '_' | '*') {
            cur.extend(&code[i..(i + 2).min(code.len())]);
            i += 2;
            continue;
        }
        if ch == ';' {
            out.push(std::mem::take(&mut cur));
            i += 1;
            continue;
        }
        cur.push(ch);
        i += 1;
    }
    out.push(cur);
    out
}

fn is_placeholder(c: char) -> bool {
    matches!(c, '0' | '#' | '?')
}

/// One section with one decimal more (+1) or less (−1); without digits, unchanged.
fn adjust_section(sec: &str, delta: i32) -> String {
    let sc: Vec<char> = sec.chars().collect();
    // positions "of code" (outside literals): where the digits and the decimal point are
    let mut code: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < sc.len() {
        let ch = sc[i];
        if ch == '"' || ch == '[' {
            let close = if ch == '"' { '"' } else { ']' };
            let j = sc.iter().skip(i + 1).position(|&c| c == close).map(|k| i + 1 + k);
            i = j.unwrap_or(sc.len()) + 1;
            continue;
        }
        if matches!(ch, '\\' | '_' | '*') {
            i += 2;
            continue;
        }
        code.push(i);
        i += 1;
    }
    let digits: Vec<usize> = code.iter().copied().filter(|&i| is_placeholder(sc[i])).collect();
    // no digits (text, @) or fraction: Excel does not touch it
    if digits.is_empty() || code.iter().any(|&i| sc[i] == '/') {
        return sec.to_owned();
    }
    let dot = code.iter().copied().find(|&i| sc[i] == '.' && digits.iter().any(|&d| d < i));
    let splice = |at: usize, ins: &str| -> String {
        let mut s: String = sc[..at.min(sc.len())].iter().collect();
        s.push_str(ins);
        s.extend(&sc[at.min(sc.len())..]);
        s
    };
    let Some(dot) = dot else {
        if delta < 0 {
            return sec.to_owned();
        }
        // no decimals: ".0" after the last digit before the exponent (0% → 0.0%, #,##0 → #,##0.0)
        let exp = code.iter().copied().find(|&i| matches!(sc[i], 'e' | 'E') && matches!(sc.get(i + 1), Some('+' | '-')));
        let last = digits.iter().copied().rfind(|&d| exp.is_none_or(|e| d < e)).unwrap_or(digits[digits.len() - 1]);
        return splice(last + 1, ".0");
    };
    // decimals: the run of digits right after the point
    let mut k = code.iter().position(|&i| i == dot).unwrap_or(0) + 1;
    let mut dec: Vec<usize> = Vec::new();
    while k < code.len() && is_placeholder(sc[code[k]]) && code[k] == dec.last().map_or(dot + 1, |l| l + 1) {
        dec.push(code[k]);
        k += 1;
    }
    if delta > 0 {
        let at = dec.last().map_or(dot + 1, |l| l + 1);
        return splice(at, "0");
    }
    if dec.len() <= 1 {
        let mut s: String = sc[..dot].iter().collect();
        s.extend(&sc[(dot + 1 + dec.len()).min(sc.len())..]);
        return s;
    }
    let at = dec[dec.len() - 1];
    let mut s: String = sc[..at].iter().collect();
    s.extend(&sc[at + 1..]);
    s
}

/// Increase (+1) / Decrease (−1) Decimal places over the current code, in every section. `General` becomes `0.0` /
/// `0` (Excel starts from the displayed places; here from none); text (`@`) does not change.
#[must_use]
pub fn adjust_decimals(code: &str, delta: i32) -> String {
    if code.is_empty() || uo_jsnum::text::trim(code).eq_ignore_ascii_case("general") {
        return if delta > 0 { "0.0" } else { "0" }.to_owned();
    }
    let chars: Vec<char> = code.chars().collect();
    sections(&chars).iter().map(|s| adjust_section(s, delta)).collect::<Vec<_>>().join(";")
}
