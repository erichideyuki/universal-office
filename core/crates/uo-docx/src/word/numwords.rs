//! Word's number words (WordCraft study, area "fields", item B22/B26; record `core/docs/modules/uo-docx-layout.md`
//! "WordCraft study — fields"): the `\* CardText`, `\* OrdText`, `\* Ordinal` and `\* DollarText` results of a field and
//! the `cardinalText`/`ordinalText`/`ordinal` list numbers, in pt-BR, en-US and es-ES, as Word 365 for Windows writes
//! them. Measured through `Fields.Update` (46,469 values: every N in 1..1100, every thousands head g·1000 and g·1000+7
//! for g = 1..999, every low part after g = 1, 22, 371, plus 600 log-uniform values; `tests/oracle_wc/numwords` of
//! uo-docx-layout): every value is reproduced, Word's own quirks included ("Cinqüenta", "Quinientosun mil", the
//! leading space of es " tricentésimo").
//!
//! Composition (verified on g = 1, 22, 371 × l = 1..999): `words(g·1000 + l) = Head(g, l > 0) + Tail(l)`. Values from
//! 1,000,000 up are not representable (Word's error, decided by the caller). Languages other than pt/es take the
//! English words (Word's UI tables for other languages are not known here).

/// The language family of the words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordsLang {
    Pt,
    En,
    Es,
}

impl WordsLang {
    /// `pt*` → Portuguese, `es*` → Spanish, anything else → English (ASCII case-insensitive).
    #[must_use]
    pub fn of(lang: &str) -> WordsLang {
        let p = lang.as_bytes().get(..2).map(<[u8]>::to_ascii_lowercase);
        match p.as_deref() {
            Some(b"pt") => WordsLang::Pt,
            Some(b"es") => WordsLang::Es,
            _ => WordsLang::En,
        }
    }
}

/// The first letter upper-cased (`s[0].toUpperCase() + s.slice(1)`).
#[must_use]
pub fn cap_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

fn ix(n: u32) -> usize {
    usize::try_from(n).unwrap_or(0)
}

fn at<'a>(t: &[&'a str], n: u32) -> &'a str {
    t.get(ix(n)).copied().unwrap_or("")
}

// ── pt-BR ──
const PT_U: [&str; 20] = [
    "zero",
    "um",
    "dois",
    "três",
    "quatro",
    "cinco",
    "seis",
    "sete",
    "oito",
    "nove",
    "dez",
    "onze",
    "doze",
    "treze",
    "quatorze",
    "quinze",
    "dezesseis",
    "dezessete",
    "dezoito",
    "dezenove",
];
const PT_T: [&str; 10] = ["", "", "vinte", "trinta", "quarenta", "cinqüenta", "sessenta", "setenta", "oitenta", "noventa"];
const PT_H: [&str; 10] =
    ["", "cento", "duzentos", "trezentos", "quatrocentos", "quinhentos", "seiscentos", "setecentos", "oitocentos", "novecentos"];
const PT_OU: [&str; 10] = ["zero", "primeiro", "segundo", "terceiro", "quarto", "quinto", "sexto", "sétimo", "oitavo", "nono"];
const PT_OT: [&str; 10] =
    ["", "décimo", "vigésimo", "trigésimo", "quadragésimo", "qüinquagésimo", "sexagésimo", "setuagésimo", "octogésimo", "nonagésimo"];
const PT_OH: [&str; 10] = [
    "",
    "centésimo",
    "ducentésimo",
    "trecentésimo",
    "quadringentésimo",
    "qüingentésimo",
    "sexcentésimo",
    "setingentésimo",
    "octingentésimo",
    "nongentésimo",
];

fn pt_below100(n: u32) -> String {
    if n < 20 {
        return at(&PT_U, n).to_owned();
    }
    let mut s = at(&PT_T, n / 10).to_owned();
    if !n.is_multiple_of(10) {
        s.push_str(" e ");
        s.push_str(at(&PT_U, n % 10));
    }
    s
}

fn pt_card(n: u32, join_e: bool) -> String {
    if n < 100 {
        return pt_below100(n);
    }
    if n == 100 {
        return "cem".into();
    }
    let (h, r) = (n / 100, n % 100);
    let mut s = at(&PT_H, h).to_owned();
    if r != 0 {
        s.push_str(if join_e { " e " } else { " " });
        s.push_str(&pt_below100(r));
    }
    s
}

fn pt_ord(n: u32) -> String {
    if n == 0 {
        return "zero".into();
    }
    let (h, t, u) = (n / 100, (n % 100) / 10, n % 10);
    [at(&PT_OH, h), at(&PT_OT, t), if u != 0 { at(&PT_OU, u) } else { "" }]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The thousands group as Word writes it before "mil": the hundreds and the rest WITHOUT "e" (except g < 100 and x01),
/// "cem" for 100.
fn pt_group(g: u32) -> String {
    if g < 100 {
        pt_below100(g)
    } else if g == 100 {
        "cem".into()
    } else {
        pt_card(g, g % 100 == 1)
    }
}

fn pt(n: u32, ord: bool) -> String {
    if n < 1000 {
        return cap_first(&if ord { pt_ord(n) } else { pt_card(n, true) });
    }
    let (g, l) = (n / 1000, n % 1000);
    let head = if ord {
        if g == 1 {
            "milésimo".to_owned()
        } else if g > 100 && g % 100 == 1 {
            format!("{} milésimo", at(&PT_H, g / 100))
        } else {
            format!("{} milésimos", pt_group(g))
        }
    } else if g == 1 {
        "mil".to_owned()
    } else {
        format!("{} mil", pt_group(g))
    };
    if l == 0 {
        return cap_first(&head);
    }
    // the low part after the thousands: "e" before l < 100 and before whole hundreds ("cento" for 100)
    let whole = l % 100 == 0;
    if ord {
        return cap_first(&format!("{head} {}{}", if whole { "e " } else { "" }, pt_ord(l)));
    }
    if l < 100 || whole {
        return cap_first(&format!("{head} e {}", if l == 100 { "cento".to_owned() } else { pt_card(l, true) }));
    }
    cap_first(&format!("{head} {}", pt_card(l, true)))
}

// ── en-US ──
const EN_U: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const EN_T: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];

fn en_card3(n: u32) -> String {
    let (h, r) = (n / 100, n % 100);
    let low = if r < 20 {
        if r != 0 || h == 0 { at(&EN_U, r).to_owned() } else { String::new() }
    } else {
        let mut s = at(&EN_T, r / 10).to_owned();
        if !r.is_multiple_of(10) {
            s.push('-');
            s.push_str(at(&EN_U, r % 10));
        }
        s
    };
    let hund = if h != 0 { format!("{} hundred", at(&EN_U, h)) } else { String::new() };
    [hund, low].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

fn en_card(n: u32) -> String {
    if n < 1000 {
        return en_card3(n);
    }
    let mut s = format!("{} thousand", en_card3(n / 1000));
    if !n.is_multiple_of(1000) {
        s.push(' ');
        s.push_str(&en_card3(n % 1000));
    }
    s
}

fn en_ord(n: u32) -> String {
    let c = en_card(n);
    // `/^(.*?)([a-z]+)$/`: the trailing run of letters is the last word (a hyphen or a space ends it)
    let split = c.char_indices().rev().find(|(_, ch)| !ch.is_ascii_lowercase()).map_or(0, |(i, ch)| i + ch.len_utf8());
    let (pre, last) = c.split_at(split);
    let o = match last {
        "one" => "first".to_owned(),
        "two" => "second".to_owned(),
        "three" => "third".to_owned(),
        "five" => "fifth".to_owned(),
        "eight" => "eighth".to_owned(),
        "nine" => "ninth".to_owned(),
        "twelve" => "twelfth".to_owned(),
        l if l.ends_with('y') => format!("{}ieth", &l[..l.len() - 1]),
        l => format!("{l}th"),
    };
    format!("{pre}{o}")
}

// ── es-ES ──
const ES_U: [&str; 30] = [
    "cero",
    "uno",
    "dos",
    "tres",
    "cuatro",
    "cinco",
    "seis",
    "siete",
    "ocho",
    "nueve",
    "diez",
    "once",
    "doce",
    "trece",
    "catorce",
    "quince",
    "dieciséis",
    "diecisiete",
    "dieciocho",
    "diecinueve",
    "veinte",
    "veintiuno",
    "veintidós",
    "veintitrés",
    "veinticuatro",
    "veinticinco",
    "veintiséis",
    "veintisiete",
    "veintiocho",
    "veintinueve",
];
const ES_T: [&str; 10] = ["", "", "", "treinta", "cuarenta", "cincuenta", "sesenta", "setenta", "ochenta", "noventa"];
const ES_H: [&str; 10] =
    ["", "ciento", "doscientos", "trescientos", "cuatrocientos", "quinientos", "seiscientos", "setecientos", "ochocientos", "novecientos"];
const ES_OU: [&str; 10] = ["cero", "primero", "segundo", "tercero", "cuarto", "quinto", "sexto", "séptimo", "octavo", "noveno"];
const ES_O1X: [&str; 10] = [
    "décimo",
    "undécimo",
    "duodécimo",
    "decimotercero",
    "decimocuarto",
    "decimoquinto",
    "decimosexto",
    "decimoséptimo",
    "decimoctavo",
    "decimonoveno",
];
const ES_OT: [&str; 10] =
    ["", "", "vigésimo", "trigésimo", "cuadragésimo", "quincuagésimo", "sexagésimo", "septuagésimo", "octogésimo", "nonagésimo"];
// (Word's own: 300 and 400 with a leading space)
const ES_OH: [&str; 10] = [
    "",
    "centésimo",
    "ducentésimo",
    " tricentésimo",
    " cuadringentésimo",
    "quingentésimo",
    "sexcentésimo",
    "septingentésimo",
    "octingentésimo",
    "noningentésimo",
];
const ES_HO: [&str; 10] = [
    "",
    "ciento",
    "doscientos",
    "trescientos",
    "cuatrocientos",
    "quincientos",
    "seiscientos",
    "sietecientos",
    "ochocientos",
    "nuevecientos",
];
const ES_V: [&str; 10] = [
    "veinte",
    "veintiuno",
    "veintidos",
    "veintitres",
    "veinticuatro",
    "veinticinco",
    "veintiseis",
    "veintisiete",
    "veintiocho",
    "veintinueve",
];

fn es_below100(n: u32) -> String {
    if n < 30 {
        return at(&ES_U, n).to_owned();
    }
    let mut s = at(&ES_T, n / 10).to_owned();
    if !n.is_multiple_of(10) {
        s.push_str(" y ");
        s.push_str(at(&ES_U, n % 10));
    }
    s
}

fn es_card(n: u32) -> String {
    if n == 0 {
        return "cero".into();
    }
    let (h, r) = (n / 100, n % 100);
    let low = if r != 0 { es_below100(r) } else { String::new() };
    [at(&ES_H, h).to_owned(), low].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Collapses runs of spaces to one (`/  +/g` → ' ').
fn squeeze(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev = false;
    for c in s.chars() {
        if c == ' ' {
            if !prev {
                out.push(c);
            }
            prev = true;
        } else {
            out.push(c);
            prev = false;
        }
    }
    out
}

fn es_ord(n: u32) -> String {
    if n == 0 {
        return "cero".into();
    }
    let (h, r) = (n / 100, n % 100);
    let (t, u) = (r / 10, r % 10);
    let low = if r == 0 {
        String::new()
    } else if r < 10 {
        at(&ES_OU, r).to_owned()
    } else if r < 20 {
        at(&ES_O1X, r - 10).to_owned()
    } else {
        let mut s = at(&ES_OT, t).to_owned();
        if u != 0 {
            s.push(' ');
            s.push_str(at(&ES_OU, u));
        }
        s
    };
    let parts: Vec<String> = [at(&ES_OH, h).to_owned(), low].into_iter().filter(|s| !s.is_empty()).collect();
    squeeze(&parts.join(" "))
}

fn es_card_head(g: u32, l: u32) -> String {
    if g == 1 {
        return "mil".into();
    }
    if g == 100 {
        return "cien mil".into();
    }
    let (h, r) = (g / 100, g % 100);
    if r == 0 {
        return format!("{} mil", at(&ES_H, h));
    }
    let rw = if r == 1 && h != 0 { "un".to_owned() } else { es_below100(r) };
    let sep = if h == 5 && r == 1 && l == 0 { "" } else { " " };
    let glue = if matches!(r, 22 | 23 | 26) { "" } else { " " };
    let hund = if h != 0 { format!("{}{sep}", at(&ES_H, h)) } else { String::new() };
    format!("{hund}{rw}{glue}mil")
}

fn es_ord_head(g: u32) -> String {
    if g == 1 {
        return "milésimo".into();
    }
    if g == 100 {
        return "cienmilésimo".into();
    }
    let (h, r) = (g / 100, g % 100);
    let (t, u) = (r / 10, r % 10);
    let rw = if r == 0 {
        String::new()
    } else if r == 1 {
        if h != 0 { "un".into() } else { String::new() }
    } else if r < 20 {
        at(&ES_U, r).to_owned()
    } else if r == 20 {
        "veinte ".into()
    } else if r < 30 {
        if r == 21 { "veinti".into() } else { at(&ES_V, u).to_owned() }
    } else {
        format!(
            "{}{}",
            at(&ES_T, t),
            match u {
                0 => " ".to_owned(),
                1 => " y ".to_owned(),
                _ => format!(" y {}", at(&ES_U, u)),
            }
        )
    };
    format!("{}{rw}milésimo", at(&ES_HO, h))
}

fn es(n: u32, ord: bool) -> String {
    if n < 1000 {
        let s = if ord { es_ord(n) } else { es_card(n) };
        return if s.starts_with(' ') { s } else { cap_first(&s) };
    }
    let (g, l) = (n / 1000, n % 1000);
    let head = if ord { es_ord_head(g) } else { es_card_head(g, l) };
    if l == 0 {
        return cap_first(&head);
    }
    let tail = if ord { uo_jsnum::text::trim(&es_ord(l)).to_owned() } else { es_card(l) };
    cap_first(&squeeze(&format!("{head} {tail}")))
}

/// The largest value Word writes in words (`\* CardText`, `OrdText`, `DollarText`): 999,999.
pub const MAX_WORDS: u32 = 999_999;

/// `\* CardText` of `n` (0 ≤ n ≤ 999,999) in Word's case (en lower case, pt/es with a capital first letter); `None`
/// above [`MAX_WORDS`].
#[must_use]
pub fn card_text(n: u32, lang: WordsLang) -> Option<String> {
    (n <= MAX_WORDS).then(|| match lang {
        WordsLang::Pt => pt(n, false),
        WordsLang::En => en_card(n),
        WordsLang::Es => es(n, false),
    })
}

/// `\* OrdText` of `n` (0 ≤ n ≤ 999,999); `None` above [`MAX_WORDS`].
#[must_use]
pub fn ord_text(n: u32, lang: WordsLang) -> Option<String> {
    (n <= MAX_WORDS).then(|| match lang {
        WordsLang::Pt => pt(n, true),
        WordsLang::En => en_ord(n),
        WordsLang::Es => es(n, true),
    })
}

/// The integer a JS number holds when it is one below 2^53 in magnitude, else 0 — rebuilt from two `ToUint32` halves
/// (the workspace bans float → integer `as` casts; the callers pass integers below 9e15).
#[must_use]
pub fn int_of(x: f64) -> i64 {
    const TWO_32: f64 = 4_294_967_296.0;
    if !uo_jsnum::conv::is_integer(x) || uo_jsnum::math::abs(x) >= 9_007_199_254_740_992.0 {
        return 0;
    }
    let a = uo_jsnum::math::abs(x);
    let hi = uo_jsnum::math::floor(a / TWO_32);
    let lo = a - hi * TWO_32;
    let v = (i64::from(uo_jsnum::conv::to_uint32(hi)) << 32) | i64::from(uo_jsnum::conv::to_uint32(lo));
    if x < 0.0 { -v } else { v }
}

/// `\* Ordinal` of an integer `n` (any sign, any size): en `st/nd/rd/th` (11–13 → th), pt and es `º` (U+00BA); a
/// negative keeps its `-` (`-3rd`, `-3º`).
#[must_use]
pub fn ordinal(n: i64, lang: WordsLang) -> String {
    ordinal_text(&n.to_string(), lang)
}

/// [`ordinal`] of an integer given as its decimal text (an optional `-`, then digits — also past the exact integers of
/// an `f64`): the suffix follows the last two digits.
#[must_use]
pub fn ordinal_text(n: &str, lang: WordsLang) -> String {
    match lang {
        WordsLang::Pt | WordsLang::Es => format!("{n}º"),
        WordsLang::En => {
            let mut last = n.bytes().rev().take_while(u8::is_ascii_digit).take(2).map(|b| u64::from(b - b'0'));
            let a = last.next().unwrap_or(0) + 10 * last.next().unwrap_or(0);
            let suf = if (11..=13).contains(&(a % 100)) {
                "th"
            } else {
                match a % 10 {
                    1 => "st",
                    2 => "nd",
                    3 => "rd",
                    _ => "th",
                }
            };
            format!("{n}{suf}")
        }
    }
}

/// `\* DollarText`: the cardinal of the integer part, then "and NN/100" (en) or "con NN/100" (es). pt-BR is `None`:
/// Word 365 pt-BR crashes on it (RPC 0x800706BE, measured), so the caller keeps the cached result.
#[must_use]
pub fn dollar_text(int: u32, cents: u32, lang: WordsLang) -> Option<String> {
    let word = match lang {
        WordsLang::En => "and",
        WordsLang::Es => "con",
        WordsLang::Pt => return None,
    };
    Some(format!("{} {word} {:02}/100", card_text(int, lang)?, cents % 100))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_of_is_exact_below_2_53() {
        for v in [0i64, 1, -1, 999_999, 4_294_967_296, -8_999_999_999_999_999 / 1000, 9_007_199_254_740_991] {
            #[allow(clippy::cast_precision_loss)]
            let x = v as f64;
            assert_eq!(int_of(x), v);
        }
        assert_eq!(int_of(1.5), 0);
        assert_eq!(int_of(f64::NAN), 0);
        assert_eq!(int_of(1e300), 0);
    }

    #[test]
    fn spot_values() {
        assert_eq!(card_text(21, WordsLang::En).as_deref(), Some("twenty-one"));
        assert_eq!(ord_text(0, WordsLang::En).as_deref(), Some("zeroth"));
        assert_eq!(ord_text(999_999, WordsLang::En).as_deref(), Some("nine hundred ninety-nine thousand nine hundred ninety-ninth"));
        assert_eq!(card_text(53, WordsLang::Pt).as_deref(), Some("Cinqüenta e três"));
        assert_eq!(card_text(2053, WordsLang::Pt).as_deref(), Some("Dois mil e cinqüenta e três"));
        assert_eq!(card_text(1100, WordsLang::Pt).as_deref(), Some("Mil e cento"));
        assert_eq!(card_text(104_000, WordsLang::Pt).as_deref(), Some("Cento quatro mil"));
        assert_eq!(card_text(101_000, WordsLang::Pt).as_deref(), Some("Cento e um mil"));
        assert_eq!(ord_text(999_999, WordsLang::Pt).as_deref(), Some("Novecentos noventa e nove milésimos nongentésimo nonagésimo nono"));
        assert_eq!(card_text(21, WordsLang::Es).as_deref(), Some("Veintiuno"));
        assert_eq!(card_text(501_000, WordsLang::Es).as_deref(), Some("Quinientosun mil"));
        assert_eq!(card_text(501_713, WordsLang::Es).as_deref(), Some("Quinientos un mil setecientos trece"));
        assert_eq!(ord_text(300, WordsLang::Es).as_deref(), Some(" tricentésimo"));
        assert_eq!(
            ord_text(999_999, WordsLang::Es).as_deref(),
            Some("Nuevecientosnoventa y nuevemilésimo noningentésimo nonagésimo noveno")
        );
        assert_eq!(card_text(1_000_000, WordsLang::En), None);
        assert_eq!(ordinal(112, WordsLang::En), "112th");
        assert_eq!(ordinal(-3, WordsLang::En), "-3rd");
        assert_eq!(ordinal(21, WordsLang::Es), "21º");
        assert_eq!(dollar_text(2, 1, WordsLang::En).as_deref(), Some("two and 01/100"));
        assert_eq!(dollar_text(2, 1, WordsLang::Pt), None);
        assert_eq!(WordsLang::of("es-419"), WordsLang::Es);
        assert_eq!(WordsLang::of("PT"), WordsLang::Pt);
        assert_eq!(WordsLang::of("de-DE"), WordsLang::En);
    }
}
