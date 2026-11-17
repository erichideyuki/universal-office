//! Section emission of `numfmt.ts`: decor tokens, digits, scientific, fractions, text and the
//! merge of parts.

use uo_jsnum::format::to_js_string;
use uo_jsnum::math;

use crate::digits::decimal_digits;
use crate::locale::FormatLocale;
use crate::parse::{Section, Tok, U, u};

/// A piece of the formatted text (`FmtPart`). Texts and characters are UTF-16 code units: the TS
/// indexes `raw[i + 1]` by code unit, so `_` or `*` before a supplementary character takes only its
/// high surrogate and the low half stays in the next literal.
#[derive(Clone, Debug, PartialEq)]
pub enum FmtPart {
    /// text
    Text(Vec<u16>),
    /// `_x`: the width of x
    Skip(u16),
    /// `*x`: repeat x to fill the column
    Fill(u16),
    /// a number in General format whose precision depends on the column width
    General(f64),
}

impl FmtPart {
    fn text(s: &str) -> FmtPart {
        FmtPart::Text(u(s))
    }
}

pub(crate) fn push_decor(t: &Tok, out: &mut Vec<FmtPart>) {
    match t {
        Tok::Lit { s, .. } => out.push(FmtPart::Text(s.clone())),
        Tok::Skip(c) => out.push(FmtPart::Skip(*c)),
        Tok::Fill(c) => out.push(FmtPart::Fill(*c)),
        _ => {}
    }
}

/// Only the LAST `*x` counts.
pub(crate) fn last_fill_only(out: Vec<FmtPart>) -> Vec<FmtPart> {
    let last = out.iter().rposition(|p| matches!(p, FmtPart::Fill(_)));
    match last {
        None => out,
        Some(l) => out.into_iter().enumerate().filter(|(i, p)| !matches!(p, FmtPart::Fill(_)) || *i == l).map(|(_, p)| p).collect(),
    }
}

pub(crate) fn merge_parts(out: Vec<FmtPart>) -> Vec<FmtPart> {
    let mut r: Vec<FmtPart> = Vec::with_capacity(out.len());
    for p in out {
        if let FmtPart::Text(s) = &p {
            if s.is_empty() {
                continue;
            }
            if let Some(FmtPart::Text(prev)) = r.last_mut() {
                prev.extend_from_slice(s);
                continue;
            }
        }
        r.push(p);
    }
    r
}

pub(crate) fn emit_text(sec: &Section, text: &[u16]) -> Vec<FmtPart> {
    let mut out = Vec::new();
    for t in &sec.toks {
        if matches!(t, Tok::At) {
            out.push(FmtPart::Text(text.to_vec()));
        } else {
            push_decor(t, &mut out);
        }
    }
    merge_parts(last_fill_only(out))
}

pub(crate) fn emit_literals(sec: &Section, out: &mut Vec<FmtPart>) {
    for t in &sec.toks {
        push_decor(t, out);
    }
}

pub(crate) fn emit_general_section(sec: &Section, v: f64, out: &mut Vec<FmtPart>) {
    for t in &sec.toks {
        if matches!(t, Tok::Gen) {
            out.push(FmtPart::General(v));
        } else {
            push_decor(t, out);
        }
    }
}

/// Emits the numeric section; returns whether the DISPLAYED value is non-zero (for the sign).
pub(crate) fn emit_number(sec: &Section, a: f64, loc: &FormatLocale, out: &mut Vec<FmtPart>) -> bool {
    let v = a * sec.scale;
    if sec.exp {
        emit_scientific(sec, v, loc, out);
        return v > 0.0;
    }
    if sec.frac.is_some() {
        return emit_fraction(sec, v, out);
    }
    let (int_str, frac_str) = decimal_digits(v, sec.n_frac);
    emit_digits(sec, &int_str, &frac_str, loc, out, sec.toks.len());
    !int_str.is_empty() || frac_str.bytes().any(|b| (b'1'..=b'9').contains(&b))
}

fn pad_unit(c: u16) -> &'static str {
    if c == u16::from(b'0') {
        "0"
    } else if c == u16::from(b'?') {
        " "
    } else {
        ""
    }
}

/// Emits the tokens `[0, end)` with the integer/fraction digits spread over the placeholders.
fn emit_digits(sec: &Section, int_str: &str, frac_str: &str, loc: &FormatLocale, out: &mut Vec<FmtPart>, end: usize) {
    let toks = &sec.toks;
    let n = sec.n_int as i64;
    let ib = int_str.as_bytes();
    let len = ib.len() as i64;
    let extra = (len - n).max(0);
    let mut int_seen = 0i64;
    let mut frac_seen = 0usize;
    let mut after_pt = false;
    // last significant fraction digit (trailing zeros follow the placeholder's rule)
    let fb = frac_str.as_bytes();
    let last_sig: i64 = fb.iter().rposition(|&b| b != b'0').map_or(-1, |p| p as i64);
    let sep_after = |pos: i64| -> &str { if sec.group && pos > 0 && pos % 3 == 0 { loc.group.as_str() } else { "" } };
    let digit =
        |k: i64| -> String { usize::try_from(k).ok().and_then(|k| ib.get(k)).map_or(String::new(), |&b| char::from(b).to_string()) };
    for t in toks.iter().take(end) {
        match t {
            Tok::Pt => {
                if n == 0 && len > 0 {
                    out.push(FmtPart::text(int_str));
                }
                out.push(FmtPart::text(&loc.decimal));
                after_pt = true;
            }
            Tok::Dig(c) => {
                if !after_pt {
                    let j = int_seen;
                    int_seen += 1;
                    let pos = n - 1 - j; // position of the placeholder (from the right)
                    let mut s = String::new();
                    if j == 0 && extra > 0 {
                        // the extra digits go into the 1st placeholder
                        for q in 0..=extra {
                            let p2 = len - 1 - q;
                            s.push_str(&digit(q));
                            s.push_str(sep_after(p2));
                        }
                    } else {
                        let k = len - 1 - pos; // index into int_str
                        if k >= 0 {
                            s.push_str(&digit(k));
                            s.push_str(sep_after(pos));
                        } else if *c == u16::from(b'0') {
                            s.push('0');
                            s.push_str(sep_after(pos));
                        } else if *c == u16::from(b'?') {
                            s.push(' ');
                            if !sep_after(pos).is_empty() {
                                s.push(' ');
                            }
                        }
                        // "#" with 0 -> "": s stays empty
                    }
                    out.push(FmtPart::text(&s));
                } else {
                    let j = frac_seen;
                    frac_seen += 1;
                    let d = fb.get(j).map_or('0', |&b| char::from(b));
                    let s: String = if (j as i64) <= last_sig { d.to_string() } else { pad_unit(*c).to_owned() };
                    out.push(FmtPart::text(&s));
                }
            }
            Tok::Pct => out.push(FmtPart::text("%")),
            _ => push_decor(t, out),
        }
    }
    if n == 0 && !toks.iter().any(|t| matches!(t, Tok::Pt)) && len > 0 {
        out.push(FmtPart::text(int_str));
    }
}

fn emit_scientific(sec: &Section, a: f64, loc: &FormatLocale, out: &mut Vec<FmtPart>) {
    let toks = &sec.toks;
    let exp_at = toks.iter().position(|t| matches!(t, Tok::Exp { .. })).unwrap_or(0);
    let plus = matches!(toks.get(exp_at), Some(Tok::Exp { plus: true }));
    // with a thousands separator the mantissa has at least 4 integer positions ("#,#" = "#,###")
    let n_int = 1usize.max(if sec.group { 4usize.max(sec.n_int) } else { sec.n_int });
    let ni = n_int as f64;
    let mut e = 0.0f64;
    let mut mant = 0.0f64;
    if a > 0.0 {
        e = math::floor(math::log10(a));
        // engineering step = number of integer placeholders (##0.0E+0 -> multiples of 3)
        if n_int > 1 {
            e = math::floor(e / ni) * ni;
        }
        mant = a / math::pow(10.0, e);
        let (ip, _) = decimal_digits(mant, sec.n_frac);
        if ip.len() > n_int {
            e += if n_int > 1 { ni } else { 1.0 };
            mant = a / math::pow(10.0, e);
        } else if n_int == 1 && ip.len() > 1 {
            e += 1.0;
            mant = a / math::pow(10.0, e);
        }
    }
    let (int_str, frac_str) = decimal_digits(mant, sec.n_frac);
    // zero: every integer placeholder becomes 0
    let int_arg = if a > 0.0 { int_str } else { "0".repeat(n_int) };
    emit_digits(sec, &int_arg, &frac_str, loc, out, exp_at);
    // E+ always signed; E- only when the exponent is negative; the sign sticks to the exponent digits
    out.push(FmtPart::text("E"));
    let sign = if e < 0.0 {
        "-"
    } else if plus {
        "+"
    } else {
        ""
    };
    let exp_toks = toks.get(exp_at + 1..).unwrap_or(&[]);
    let min_exp = exp_toks.iter().filter(|t| matches!(t, Tok::Dig(c) if *c == u16::from(b'0'))).count();
    let digits = to_js_string(e.abs());
    let es = format!("{digits:0>min_exp$}");
    let mut done = false;
    for t in exp_toks {
        if matches!(t, Tok::Dig(_)) {
            if !done {
                out.push(FmtPart::text(&format!("{sign}{es}")));
                done = true;
            }
            continue;
        }
        push_decor(t, out);
    }
    if !done && !sign.is_empty() {
        out.push(FmtPart::text(sign));
    }
}

fn ph(toks: &[Tok], i: usize) -> u16 {
    // a placeholder's character; 0 for a token that is not one (a fixed-denominator literal the TS
    // reads `.c` from, giving `undefined`)
    match toks.get(i) {
        Some(Tok::Dig(c)) => *c,
        _ => 0,
    }
}

fn has_digit(s: &str) -> bool {
    s.bytes().any(|b| b.is_ascii_digit())
}

/// `rightAlign`: digits right-aligned in the placeholders (the extra ones go in the 1st).
fn right_align(digits: &str, p: &[u16]) -> Vec<String> {
    let d = digits.as_bytes();
    let n = p.len();
    let len = d.len();
    p.iter()
        .enumerate()
        .map(|(j, &c)| {
            if j == 0 && len > n {
                return d.get(..len - n + 1).map_or(String::new(), |s| s.iter().map(|&b| char::from(b)).collect());
            }
            match (len + j).checked_sub(n).and_then(|k| d.get(k)) {
                Some(&b) => char::from(b).to_string(),
                None => pad_unit(c).to_owned(),
            }
        })
        .collect()
}

/// Denominator: the digits take the `?` from the left and, when short, the non-`?` from the right.
fn den_of(den_ph: &[u16], d: &str) -> Vec<String> {
    let n = den_ph.len();
    let db = d.as_bytes();
    let mut slots = vec![false; n];
    let mut count = 0usize;
    let q = u16::from(b'?');
    for j in 0..n {
        if count >= db.len() {
            break;
        }
        if den_ph.get(j) == Some(&q) {
            if let Some(s) = slots.get_mut(j) {
                *s = true;
            }
            count += 1;
        }
    }
    for j in (0..n).rev() {
        if count >= db.len() {
            break;
        }
        if den_ph.get(j) != Some(&q) && slots.get(j) == Some(&false) {
            if let Some(s) = slots.get_mut(j) {
                *s = true;
            }
            count += 1;
        }
    }
    let order: Vec<usize> = (0..n).filter(|&j| slots.get(j) == Some(&true)).collect();
    let mut res: Vec<String> = den_ph.iter().map(|&c| pad_unit(c).to_owned()).collect();
    for (qi, &j) in order.iter().enumerate() {
        let piece: String = if qi + 1 == order.len() {
            db.get(qi..).map_or(String::new(), |s| s.iter().map(|&b| char::from(b)).collect())
        } else {
            db.get(qi).map_or(String::new(), |&b| char::from(b).to_string())
        };
        if let Some(r) = res.get_mut(j) {
            *r = piece;
        }
    }
    res
}

/// Fractions ("# ?/?", "0/x#", "#\:? ?=/=#"...): see the comment of `emitFraction` in numfmt.ts.
fn emit_fraction(sec: &Section, a: f64, out: &mut Vec<FmtPart>) -> bool {
    let Some(f) = sec.frac.as_ref() else { return false };
    let toks = &sec.toks;
    let has_int = !f.int_idx.is_empty();
    let mut whole = if has_int { math::floor(a) } else { 0.0 };
    let frac = a - whole;
    let fixed = f.fixed_den != 0.0;
    let den_digits = if fixed { 0 } else { f.den_idx.len() };
    let max_den = if fixed { f.fixed_den } else { math::pow(10.0, den_digits as f64) - 1.0 };
    let (mut best_n, best_d);
    if fixed {
        best_d = f.fixed_den;
        best_n = math::round(frac * f.fixed_den);
    } else {
        // last CONVERGENT of the continued fraction (in double) with denominator <= max: no
        // semiconvergents; without an integer part the fraction is improper
        let a0 = math::floor(frac);
        let (mut h0, mut k0, mut h1, mut k1) = (1.0f64, 0.0f64, a0, 1.0f64);
        let mut r = frac - a0;
        for _it in 0..64 {
            if r.is_nan() || r <= 0.0 {
                break;
            }
            let y = 1.0 / r;
            let an = math::floor(y);
            r = y - an;
            let h2 = an * h1 + h0;
            let k2 = an * k1 + k0;
            if !k2.is_finite() || k2 > max_den {
                break;
            }
            h0 = h1;
            k0 = k1;
            h1 = h2;
            k1 = k2;
        }
        best_n = h1;
        best_d = k1;
    }
    if has_int && best_n >= best_d && best_d > 0.0 {
        whole += math::floor(best_n / best_d);
        best_n %= best_d;
    }
    let int_ph: Vec<u16> = f.int_idx.iter().map(|&i| ph(toks, i)).collect();
    let num_ph: Vec<u16> = f.num_idx.iter().map(|&i| ph(toks, i)).collect();
    let den_ph: Vec<u16> = if fixed { Vec::new() } else { f.den_idx.iter().map(|&i| ph(toks, i)).collect() };
    let zero_num = best_n == 0.0;
    let c0 = u16::from(b'0');
    let cq = u16::from(b'?');
    let last_int_q = has_int && int_ph.last() == Some(&cq);
    // the fraction disappears (numerator 0 without a '0' placeholder)
    let suppress = has_int && zero_num && !num_ph.contains(&c0);
    let spaced = last_int_q || num_ph.contains(&cq) || den_ph.first() == Some(&cq);
    // integer part
    let mut int_out: Vec<String> = Vec::new();
    if has_int {
        if whole > 0.0 {
            int_out = right_align(&to_js_string(whole), &int_ph);
        } else {
            int_out = int_ph.iter().map(|&c| pad_unit(c).to_owned()).collect();
            let last = int_out.len().saturating_sub(1);
            let num0_not_zero = num_ph.first() != Some(&c0);
            if zero_num
                && (suppress || last_int_q || num0_not_zero)
                && int_out.get(last).is_some_and(|s| !has_digit(s))
                && let Some(l) = int_out.get_mut(last)
            {
                *l = "0".to_owned();
            }
        }
    }
    let int_shows_digit = has_int && int_out.last().is_some_and(|s| has_digit(s));
    let last_int: i64 = if has_int { f.int_idx.last().map_or(-1, |&x| x as i64) } else { -1 };
    let num_from = f.num_idx.first().copied().unwrap_or(0);
    let frac_to = f.den_idx.last().copied().unwrap_or(0);
    let num_out = right_align(&to_js_string(best_n), &num_ph);
    let den_out: Vec<String> = if fixed { Vec::new() } else { den_of(&den_ph, &to_js_string(best_d)) };
    // in the suppression, numerator and denominator take the width of "0" and "1"
    let num_zero = if suppress { right_align("0", &num_ph) } else { num_out.clone() };
    let den_one = if suppress && !fixed { den_of(&den_ph, "1") } else { den_out.clone() };
    let fixed_text = to_js_string(f.fixed_den);
    for (i, t) in toks.iter().enumerate() {
        let ii = i as i64;
        if suppress && ii > last_int && i <= frac_to {
            if !spaced {
                if matches!(t, Tok::Dig(c) if *c == cq) && f.den_idx.contains(&i) {
                    out.push(FmtPart::text(" "));
                }
                continue;
            }
            let s: U = match t {
                Tok::Dig(c) => {
                    let ni = f.num_idx.iter().position(|&x| x == i);
                    let di = f.den_idx.iter().position(|&x| x == i);
                    if let Some(ni) = ni {
                        u(num_zero.get(ni).map_or("", String::as_str))
                    } else if let Some(di) = di {
                        u(den_one.get(di).map_or("", String::as_str))
                    } else {
                        u(pad_unit(*c))
                    }
                }
                Tok::Slash => u("/"),
                Tok::Lit { s, .. } => {
                    if fixed && f.den_idx.contains(&i) {
                        u(&fixed_text)
                    } else {
                        s.clone()
                    }
                }
                Tok::Skip(_) => u(" "),
                _ => {
                    push_decor(t, out);
                    continue;
                }
            };
            out.push(FmtPart::Text(vec![u16::from(b' '); s.len()]));
            continue;
        }
        if has_int && ii > last_int && i < num_from {
            // separator between the integer and the numerator
            if int_shows_digit {
                push_decor(t, out);
            } else if last_int_q || num_ph.first() == Some(&cq) {
                match t {
                    Tok::Lit { s, .. } => out.push(FmtPart::Text(vec![u16::from(b' '); s.len()])),
                    Tok::Skip(_) | Tok::Dig(_) => out.push(FmtPart::text(" ")),
                    _ => push_decor(t, out),
                }
            }
            continue;
        }
        if matches!(t, Tok::Dig(_)) && f.int_idx.contains(&i) {
            let k = f.int_idx.iter().position(|&x| x == i).unwrap_or(0);
            out.push(FmtPart::text(int_out.get(k).map_or("", String::as_str)));
            continue;
        }
        if matches!(t, Tok::Dig(_)) && f.num_idx.contains(&i) {
            let k = f.num_idx.iter().position(|&x| x == i).unwrap_or(0);
            out.push(FmtPart::text(num_out.get(k).map_or("", String::as_str)));
            continue;
        }
        if matches!(t, Tok::Slash) {
            out.push(FmtPart::text("/"));
            continue;
        }
        if f.den_idx.contains(&i) {
            if fixed {
                out.push(FmtPart::text(&fixed_text));
                continue;
            }
            let k = f.den_idx.iter().position(|&x| x == i).unwrap_or(0);
            out.push(FmtPart::text(den_out.get(k).map_or("", String::as_str)));
            continue;
        }
        if matches!(t, Tok::Pct) {
            out.push(FmtPart::text("%"));
            continue;
        }
        if let Tok::Dig(c) = t {
            out.push(FmtPart::text(pad_unit(*c)));
            continue;
        }
        push_decor(t, out);
    }
    whole > 0.0 || best_n > 0.0
}
