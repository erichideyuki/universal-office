//! `decimalDigits` and the General format (`formatGeneral`) of `numfmt.ts`.

use uo_jsnum::format::to_precision;
use uo_jsnum::math;
use uo_jsnum::parse::to_number;

use crate::locale::FormatLocale;

/// Budget of [`format_general`] in a wide column (the TS default `budget = 11`).
pub const DEFAULT_GENERAL_BUDGET: i32 = 11;

/// [`format_general`] clamps the budget to this: a finite double has at most 309 integer digits and
/// 15 significant ones, so for any budget from about 400 up the text is the same; the TS spends
/// `budget` characters on a zero run (and throws a `RangeError` past 2^29), Rust would allocate them.
pub const MAX_GENERAL_BUDGET: i32 = 4096;

/// `s.slice(a, b)` of a byte vector for non-negative bounds.
fn slice(v: &[u8], a: usize, b: usize) -> &[u8] {
    let b = b.min(v.len());
    let a = a.min(b);
    v.get(a..b).unwrap_or(&[])
}

fn ascii(v: &[u8]) -> String {
    // the digit vectors only ever hold ASCII digits
    v.iter().map(|&b| char::from(b)).collect()
}

/// Digits of `v` (>= 0) with `n_frac` decimal places, as Excel: only 15 significant digits (the
/// rest become 0) and "half away from zero" rounding over that decimal representation (0.005 ->
/// 0.01; 2.5 -> 3). Returns the integer part WITHOUT leading zeros (`""` for 0) and the fraction
/// part with `n_frac` digits. Both strings are ASCII digits.
#[must_use]
pub fn decimal_digits(v: f64, n_frac: usize) -> (String, String) {
    if v.is_nan() || v <= 0.0 || !v.is_finite() {
        return (String::new(), "0".repeat(n_frac));
    }
    let Ok(s) = to_precision(v, 15) else { return (String::new(), "0".repeat(n_frac)) };
    let sb = s.as_bytes();
    let ei = sb.iter().position(|&b| b == b'e');
    let mant = ei.map_or(sb, |e| slice(sb, 0, e));
    // parseInt(s.slice(ei + 1), 10): "+21", "-7"
    let exp: i64 = ei.map_or(0, |e| {
        let t = slice(sb, e + 1, sb.len());
        let (neg, d) = match t.first() {
            Some(b'-') => (true, slice(t, 1, t.len())),
            Some(b'+') => (false, slice(t, 1, t.len())),
            _ => (false, t),
        };
        let n = d.iter().take_while(|b| b.is_ascii_digit()).fold(0i64, |a, &b| a * 10 + i64::from(b - b'0'));
        if neg { -n } else { n }
    });
    let dot = mant.iter().position(|&b| b == b'.');
    let mut digits: Vec<u8> = match dot {
        Some(d) => [slice(mant, 0, d), slice(mant, d + 1, mant.len())].concat(),
        None => mant.to_vec(),
    };
    // value = 0.d1d2... x 10^point
    let mut point = dot.unwrap_or(mant.len()) as i64 + exp;
    let mut lead = 0usize;
    while lead + 1 < digits.len() && digits.get(lead) == Some(&b'0') {
        lead += 1;
    }
    digits.drain(..lead);
    point -= lead as i64;
    let nf = n_frac as i64;
    let cut = point + nf;
    if cut < 0 {
        return (String::new(), "0".repeat(n_frac));
    }
    let cut = cut as usize;
    if cut < digits.len() {
        let up = digits.get(cut).is_some_and(|&d| d >= b'5');
        digits.truncate(cut);
        if up {
            let mut i = digits.len();
            let mut carried = true;
            while i > 0 {
                i -= 1;
                if digits.get(i) == Some(&b'9') {
                    if let Some(d) = digits.get_mut(i) {
                        *d = b'0';
                    }
                } else {
                    if let Some(d) = digits.get_mut(i) {
                        *d += 1;
                    }
                    carried = false;
                    break;
                }
            }
            if carried {
                digits.insert(0, b'1');
                point += 1;
            }
        }
    }
    let total = (point + nf) as usize;
    if digits.len() < total {
        digits.resize(total, b'0');
    }
    let (int_part, frac_part): (&[u8], Vec<u8>) = if point > 0 {
        let p = point as usize;
        (slice(&digits, 0, p), slice(&digits, p, p + n_frac).to_vec())
    } else {
        let mut f = vec![b'0'; (-point) as usize];
        f.extend_from_slice(&digits);
        f.truncate(n_frac);
        (&[], f)
    };
    let int_part = {
        let z = int_part.iter().take_while(|&&b| b == b'0').count();
        slice(int_part, z, int_part.len())
    };
    let mut frac = ascii(&frac_part);
    while frac.len() < n_frac {
        frac.push('0');
    }
    (ascii(int_part), frac)
}

fn trim_zeros_end(s: &str) -> &str {
    s.trim_end_matches('0')
}

/// General format of Excel with `budget` characters (the sign does not count; 11 in a wide column).
/// Overflows to scientific when the integer part does not fit or the decimal would round to zero;
/// returns `""` when not even the minimal scientific form fits (or `x` is not finite).
///
/// The TS `budget` is a number; here an `i32` (the callers pass whole numbers).
#[must_use]
pub fn format_general(x: f64, loc: &FormatLocale, budget: i32) -> String {
    let budget = budget.min(MAX_GENERAL_BUDGET);
    if x == 0.0 {
        return "0".to_owned();
    }
    if !x.is_finite() {
        return String::new();
    }
    let neg = x < 0.0;
    let a = x.abs();
    let body = general_body(a, loc, budget);
    if body.is_empty() {
        String::new()
    } else if neg {
        format!("-{body}")
    } else {
        body
    }
}

fn general_body(a: f64, loc: &FormatLocale, budget: i32) -> String {
    let (ip, _) = decimal_digits(a, 0);
    let int_len = i64::try_from(ip.len().max(1)).unwrap_or(i64::MAX);
    let budget64 = i64::from(budget);
    if int_len > budget64 {
        return sci_general(a, loc, budget);
    }
    let avail = budget64 - int_len; // room for "," + places
    let decimals = if avail > 1 { avail - 1 } else { 0 };
    let (i, f) = decimal_digits(a, usize::try_from(decimals).unwrap_or(0));
    let frac = trim_zeros_end(&f);
    if (i.is_empty() || i == "0") && frac.is_empty() {
        // rounded to zero but the value is not zero -> scientific (if it fits)
        let sci = sci_general(a, loc, budget);
        return if sci.is_empty() { "0".to_owned() } else { sci };
    }
    if i64::try_from(i.len()).unwrap_or(i64::MAX) > budget64 {
        return sci_general(a, loc, budget);
    }
    let dec = format!(
        "{}{}",
        if i.is_empty() { "0" } else { &i },
        if frac.is_empty() { String::new() } else { format!("{}{frac}", loc.decimal) }
    );
    if a < 1.0 {
        // narrow column: scientific wins when it represents the value BETTER than the rounded decimal
        let sci = sci_general(a, loc, budget);
        if !sci.is_empty() {
            let err = |s: &str| -> f64 { (to_number(&replace_first(s, &loc.decimal, ".")) - a).abs() };
            let es = err(&sci);
            let ed = err(&dec);
            if es < ed * (1.0 - 1e-9) || (a < 1e-4 && ed > a * 1e-12 && es <= ed * (1.0 + 1e-9)) {
                return sci;
            }
        }
    }
    dec
}

/// `s.replace(pat, rep)` with a string pattern: the first occurrence only (an empty pattern matches at 0).
fn replace_first(s: &str, pat: &str, rep: &str) -> String {
    match s.find(pat) {
        Some(at) => format!("{}{rep}{}", s.get(..at).unwrap_or(""), s.get(at + pat.len()..).unwrap_or("")),
        None => s.to_owned(),
    }
}

fn sci_general(a: f64, loc: &FormatLocale, budget: i32) -> String {
    let mut e = math::floor(math::log10(a));
    for _pass in 0..2 {
        let exp_str = format!("E{}{:0>2}", if e < 0.0 { '-' } else { '+' }, uo_jsnum::format::to_js_string(e.abs()));
        let room = i64::from(budget) - i64::try_from(exp_str.len()).unwrap_or(i64::MAX); // mantissa digits + ","
        if room < 1 {
            return String::new();
        }
        let dec = if room >= 3 { room - 2 } else { 0 };
        let (i, f) = decimal_digits(a / math::pow(10.0, e), usize::try_from(dec).unwrap_or(0));
        if i.len() > 1 {
            e += 1.0;
            continue;
        }
        let frac = trim_zeros_end(&f);
        return format!(
            "{}{}{exp_str}",
            if i.is_empty() { "0" } else { &i },
            if frac.is_empty() { String::new() } else { format!("{}{frac}", loc.decimal) }
        );
    }
    String::new()
}
