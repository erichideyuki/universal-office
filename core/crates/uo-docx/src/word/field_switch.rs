//! Word's general format switches (`\* …`) of a field result (WordCraft study, area "fields", item B22; record
//! `core/docs/modules/uo-docx-layout.md` "WordCraft study — fields"), measured in Word 365 pt-BR through
//! `Fields.Update` (`tests/oracle_wc/fields/word-fields-dump.tsv` of uo-docx-layout):
//!
//! - which: PAGE, NUMPAGES, SECTIONPAGES, SECTION, PAGEREF, SEQ and formula results take the number format; the case
//!   switches also apply to text results (REF, STYLEREF; a number format on text changes nothing);
//! - order: the LAST format switch wins; the case switches after it apply in order, those before it are dropped;
//!   MERGEFORMAT and CHARFORMAT are ignored; an unknown keyword is Word's error text;
//! - keyword case: roman/alphabetic in all lower case give lower case, any other spelling upper case; the rest are
//!   case-insensitive; Hex is upper case;
//! - values: half away from zero; negatives keep `-` (Arabic, ArabicDash, Ordinal); roman, alphabetic, Hex, CardText,
//!   OrdText and DollarText of a negative or of 1,000,000 or more are Word's error; zero: roman/alphabetic a single
//!   space, Hex "0", ArabicDash "- 0 -"; roman past 3,999 repeats M; alphabetic n = 26k + r repeats the letter.
//!
//! The error texts follow the UI language (Word's own UI: a pt-BR Word writes them in Portuguese in an en-US document,
//! measured); the en/es texts are Word's known UI strings (not measured: only pt-BR Office is installed).

use super::numwords::{self, WordsLang};

const BSL: u16 = 0x5C;
const STAR: u16 = 0x2A;

/// A number format of `\*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumFormat {
    Arabic,
    ArabicDash,
    Roman {
        upper: bool,
    },
    Alphabetic {
        upper: bool,
    },
    CardText,
    OrdText,
    Ordinal,
    DollarText,
    Hex,
    /// a format Word knows that this engine does not render (Hebrew1, DBNum1, ThaiArabic…): the number as Arabic
    Other,
}

/// A case switch of `\*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseSwitch {
    Upper,
    Lower,
    FirstCap,
    Caps,
}

/// The `\*` switches of an instruction.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeneralSwitches {
    /// the last number format
    pub format: Option<NumFormat>,
    /// the case switches after the last number format, in order
    pub cases: Vec<CaseSwitch>,
    /// a keyword Word does not know (`\* bogus`)
    pub unknown: bool,
}

impl GeneralSwitches {
    /// No switch that changes anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.format.is_none() && self.cases.is_empty() && !self.unknown
    }
}

/// Word's other general formats (accepted, not rendered here).
const OTHER: &[&str] = &[
    "aiueo",
    "arabicabjad",
    "arabicalpha",
    "bahttext",
    "chosung",
    "circlenum",
    "dbchar",
    "dbnum1",
    "dbnum2",
    "dbnum3",
    "dbnum4",
    "ganada",
    "gb1",
    "gb2",
    "gb3",
    "gb4",
    "hebrew1",
    "hebrew2",
    "hindiarabic",
    "hindicardtext",
    "hindiletter1",
    "hindiletter2",
    "iroha",
    "kanjinum1",
    "kanjinum2",
    "kanjinum3",
    "sbchar",
    "thaiarabic",
    "thaicardtext",
    "thailetter",
    "vietcardtext",
    "zodiac1",
    "zodiac2",
    "zodiac3",
];

fn is_space(u: u16) -> bool {
    uo_jsnum::text::is_js_space_unit(u)
}

/// The keywords after each `\*` of `instr` (a keyword ends at a space, a backslash or a quote).
fn keywords(instr: &[u16]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 1 < instr.len() {
        if instr.get(i) == Some(&BSL) && instr.get(i + 1) == Some(&STAR) {
            let mut k = i + 2;
            while instr.get(k).is_some_and(|&u| is_space(u)) {
                k += 1;
            }
            let s = k;
            while instr.get(k).is_some_and(|&u| !is_space(u) && u != BSL && u != 0x22) {
                k += 1;
            }
            if k > s {
                out.push(String::from_utf16_lossy(instr.get(s..k).unwrap_or(&[])));
            }
            i = k;
            continue;
        }
        i += 1;
    }
    out
}

/// The `\*` switches of a field instruction (see the module comment).
#[must_use]
pub fn parse_general(instr: &[u16]) -> GeneralSwitches {
    let mut g = GeneralSwitches::default();
    for kw in keywords(instr) {
        let lower = kw.to_ascii_lowercase();
        let all_lower = kw == lower;
        let f = match lower.as_str() {
            "mergeformat" | "charformat" => continue,
            "upper" | "lower" | "firstcap" | "caps" => {
                g.cases.push(match lower.as_str() {
                    "upper" => CaseSwitch::Upper,
                    "lower" => CaseSwitch::Lower,
                    "firstcap" => CaseSwitch::FirstCap,
                    _ => CaseSwitch::Caps,
                });
                continue;
            }
            "arabic" => NumFormat::Arabic,
            "arabicdash" => NumFormat::ArabicDash,
            "roman" => NumFormat::Roman { upper: !all_lower },
            "alphabetic" => NumFormat::Alphabetic { upper: !all_lower },
            "cardtext" => NumFormat::CardText,
            "ordtext" => NumFormat::OrdText,
            "ordinal" => NumFormat::Ordinal,
            "dollartext" => NumFormat::DollarText,
            "hex" => NumFormat::Hex,
            o if OTHER.contains(&o) => NumFormat::Other,
            _ => {
                g.unknown = true;
                continue;
            }
        };
        g.format = Some(f);
        g.cases.clear();
    }
    g
}

/// Which of Word's two errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchError {
    /// "Number cannot be represented in specified format."
    NotRepresentable,
    /// "Unknown switch argument."
    UnknownSwitch,
}

impl SwitchError {
    /// The text in the UI language (pt measured; en/es Word's UI strings, unverified).
    #[must_use]
    pub fn text(self, ui_lang: &str) -> &'static str {
        match (self, WordsLang::of(ui_lang)) {
            (SwitchError::NotRepresentable, WordsLang::Pt) => "Erro! Não é possível representar o número no formato especificado.",
            (SwitchError::NotRepresentable, WordsLang::Es) => "¡Error! No se puede representar el número en el formato especificado.",
            (SwitchError::NotRepresentable, WordsLang::En) => "Error! Number cannot be represented in specified format.",
            (SwitchError::UnknownSwitch, WordsLang::Pt) => "Erro! Argumento de opção desconhecido.",
            (SwitchError::UnknownSwitch, WordsLang::Es) => "¡Error! Argumento de modificador desconocido.",
            (SwitchError::UnknownSwitch, WordsLang::En) => "Error! Unknown switch argument.",
        }
    }
}

/// Whether `t` is one of the texts of [`SwitchError`] in any language.
#[must_use]
pub fn is_switch_error_text(t: &[u16]) -> bool {
    [SwitchError::NotRepresentable, SwitchError::UnknownSwitch]
        .iter()
        .any(|e| ["pt", "en", "es"].iter().any(|l| e.text(l).encode_utf16().eq(t.iter().copied())))
}

/// Half away from zero to an integer (`-0` → 0).
#[must_use]
pub fn round_half_away(v: f64) -> f64 {
    let r = if v.is_sign_negative() { -1.0 } else { 1.0 } * (v.abs() + 0.5).floor();
    if r == 0.0 { 0.0 } else { r }
}

const LIMIT: f64 = 1_000_000.0;

fn roman_upper(n: u64) -> String {
    const R: [(u64, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut n = n;
    let mut out = String::new();
    for (v, s) in R {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

/// The cents of `\* DollarText`: the decimal digits of the value rounded half up at two places, mod 100 (2,999 → 00,
/// the integer part not carried; measured).
fn cents_of(v: f64) -> u32 {
    let s = uo_jsnum::format::to_js_string(v.abs());
    let frac = s.split_once('.').map_or("", |(_, f)| f);
    let d: Vec<u32> = frac.chars().chain(std::iter::repeat('0')).take(3).map(|c| c.to_digit(10).unwrap_or(0)).collect();
    let (a, b, c) = (d.first().copied().unwrap_or(0), d.get(1).copied().unwrap_or(0), d.get(2).copied().unwrap_or(0));
    (a * 10 + b + u32::from(c >= 5)) % 100
}

/// The number `v` in the format of `sw` (no format: Arabic), then its case switches. `words` is the field's language
/// (its words), `ui_lang` the UI's (the error texts). `Ok(None)`: Word's result is not known (pt-BR DollarText: the
/// cached result stays). `Err` = Word's error text.
pub fn apply_number(v: f64, sw: &GeneralSwitches, words: WordsLang, ui_lang: &str) -> Result<Option<String>, &'static str> {
    if sw.unknown {
        return Err(SwitchError::UnknownSwitch.text(ui_lang));
    }
    let err = SwitchError::NotRepresentable.text(ui_lang);
    if !v.is_finite() {
        return Err(err);
    }
    let n = round_half_away(v);
    let neg = n < 0.0;
    let big = n.abs() >= LIMIT;
    // (|n| below 2^53 here, or the guarded branches return first)
    let ni = if n.abs() < 9.0e15 { numwords::int_of(n) } else { 0 };
    let nu = u32::try_from(ni.unsigned_abs()).unwrap_or(u32::MAX);
    let text = match sw.format.unwrap_or(NumFormat::Arabic) {
        NumFormat::Arabic | NumFormat::Other => uo_jsnum::format::to_js_string(n),
        NumFormat::ArabicDash => format!("- {} -", uo_jsnum::format::to_js_string(n)),
        NumFormat::Roman { upper } => {
            if neg || big {
                return Err(err);
            }
            if nu == 0 {
                " ".to_owned()
            } else {
                let r = roman_upper(u64::from(nu));
                if upper { r } else { r.to_ascii_lowercase() }
            }
        }
        NumFormat::Alphabetic { upper } => {
            if neg || big {
                return Err(err);
            }
            if nu == 0 {
                " ".to_owned()
            } else {
                let base = if upper { b'A' } else { b'a' };
                let ch = char::from(base + u8::try_from((nu - 1) % 26).unwrap_or(0));
                std::iter::repeat_n(ch, ix((nu - 1) / 26 + 1)).collect()
            }
        }
        NumFormat::Hex => {
            if neg || big {
                return Err(err);
            }
            format!("{nu:X}")
        }
        NumFormat::CardText | NumFormat::OrdText => {
            if neg || big {
                return Err(err);
            }
            let w = if sw.format == Some(NumFormat::CardText) { numwords::card_text(nu, words) } else { numwords::ord_text(nu, words) };
            w.ok_or(err)?
        }
        NumFormat::Ordinal => {
            if n.abs() < 9.0e15 {
                numwords::ordinal(ni, words)
            } else {
                // past the exact integers: the Arabic text (as `\* Arabic` writes it) with the suffix of its last
                // digits; a value JS writes with an exponent (|n| ≥ 1e21) is not representable
                let t = uo_jsnum::format::to_js_string(n);
                if !t.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
                    return Err(err);
                }
                numwords::ordinal_text(&t, words)
            }
        }
        NumFormat::DollarText => {
            let int = v.trunc();
            if v < 0.0 || int >= LIMIT {
                return Err(err);
            }
            let int = u32::try_from(numwords::int_of(int)).unwrap_or(u32::MAX);
            match numwords::dollar_text(int, cents_of(v), words) {
                Some(t) => t,
                None => return Ok(None),
            }
        }
    };
    Ok(Some(apply_case(&text, &sw.cases)))
}

fn ix(n: u32) -> usize {
    usize::try_from(n).unwrap_or(0)
}

/// The case switches in order: Upper, Lower, FirstCap (the first letter up, the rest as is), Caps (each word — after a
/// space or a hyphen — with its first letter up and the rest down: PAGE `\* Caps` over "XXI" is "Xxi", measured).
#[must_use]
pub fn apply_case(text: &str, cases: &[CaseSwitch]) -> String {
    let mut t = text.to_owned();
    for c in cases {
        t = match c {
            CaseSwitch::Upper => uo_text::case::to_uppercase_str(&t),
            CaseSwitch::Lower => uo_text::case::to_lowercase_str(&t),
            CaseSwitch::FirstCap => numwords::cap_first(&t),
            CaseSwitch::Caps => {
                let mut out = String::with_capacity(t.len());
                let mut start = true;
                for ch in t.chars() {
                    if start {
                        out.extend(ch.to_uppercase());
                    } else {
                        out.extend(ch.to_lowercase());
                    }
                    start = ch == ' ' || ch == '-' || uo_jsnum::text::is_js_space(ch);
                }
                out
            }
        };
    }
    t
}

/// The case switches of `sw` over a text result (REF, STYLEREF: a number format changes nothing; an unknown keyword is
/// the error text).
#[must_use]
pub fn apply_text(text: &[u16], sw: &GeneralSwitches, ui_lang: &str) -> Vec<u16> {
    if sw.unknown {
        return SwitchError::UnknownSwitch.text(ui_lang).encode_utf16().collect();
    }
    if sw.cases.is_empty() {
        return text.to_vec();
    }
    apply_case(&String::from_utf16_lossy(text), &sw.cases).encode_utf16().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn num(v: f64, instr: &str, l: WordsLang) -> String {
        match apply_number(v, &parse_general(&u(instr)), l, "pt-BR") {
            Ok(Some(s)) => s,
            Ok(None) => "<cache>".into(),
            Err(e) => e.into(),
        }
    }

    #[test]
    fn order_and_case() {
        let en = WordsLang::En;
        assert_eq!(num(21.0, r"\* ArabicDash \* roman", en), "xxi");
        assert_eq!(num(21.0, r"\* CardText \* Upper \* Lower", en), "twenty-one");
        assert_eq!(num(21.0, r"\* Lower \* CardText", WordsLang::Es), "Veintiuno");
        assert_eq!(num(21.0, r"\* Caps \* FirstCap", en), "21");
        assert_eq!(num(21.0, r"\* CardText \* caps", en), "Twenty-One");
        assert_eq!(num(21.0, r"\* CardText \* caps", WordsLang::Pt), "Vinte E Um");
        assert_eq!(num(21.0, r"\* Roman", en), "XXI");
        assert_eq!(num(21.0, r"\* Alphabetic", en), "U");
        assert_eq!(num(53.0, r"\* ALPHABETIC", en), "AAA");
        assert_eq!(num(52.0, r"\* alphabetic", en), "zz");
        assert_eq!(num(255.0, r"\* Hex \* Lower", en), "ff");
        assert_eq!(num(0.0, r"\* roman", en), " ");
        assert_eq!(num(-3.0, r"\* ArabicDash", en), "- -3 -");
        assert_eq!(num(-0.4, r"\* CardText", en), "zero");
        assert_eq!(num(2.5, r"\* Hex", en), "3");
        assert_eq!(num(-3.0, r"\* Ordinal", en), "-3rd");
        // past 9e15 (where the exact integer stops): the Arabic text and the suffix of its last digits, not "0th"
        assert_eq!(num(1e16, r"\* Ordinal", en), "10000000000000000th");
        assert_eq!(num(1e16, r"\* Ordinal", WordsLang::Pt), "10000000000000000º");
        assert_eq!(num(-10_000_000_000_000_002.0, r"\* Ordinal", en), "-10000000000000002nd");
        assert_eq!(num(10_000_000_000_000_012.0, r"\* Ordinal", en), "10000000000000012th");
        assert_eq!(num(1e21, r"\* Ordinal", en), "Erro! Não é possível representar o número no formato especificado.");
        assert_eq!(num(1e6, r"\* Hex", en), "Erro! Não é possível representar o número no formato especificado.");
        assert_eq!(num(21.0, r"\* bogus", en), "Erro! Argumento de opção desconhecido.");
        assert_eq!(num(21.0, r"\* MERGEFORMAT", en), "21");
        assert_eq!(num(2.005, r"\* DollarText", en), "two and 01/100");
        assert_eq!(num(2.999, r"\* DollarText", en), "two and 00/100");
        assert_eq!(
            num(999_999.99, r"\* DollarText", WordsLang::Es),
            "Novecientos noventa y nueve mil novecientos noventa y nueve con 99/100"
        );
        assert_eq!(num(14.55, r"\* DollarText", WordsLang::Pt), "<cache>");
        assert_eq!(num(32767.0, r"\* ROMAN", en), format!("{}DCCLXVII", "M".repeat(32)));
        assert_eq!(apply_case("XXI", &[CaseSwitch::Caps]), "Xxi");
        assert_eq!(apply_case("texto de Exemplo ágil-mente", &[CaseSwitch::Caps]), "Texto De Exemplo Ágil-Mente");
        assert_eq!(apply_case("texto de Exemplo ágil-mente", &[CaseSwitch::FirstCap]), "Texto de Exemplo ágil-mente");
    }
}
