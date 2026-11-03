//! String → number: `Number(s)` / unary `+s` (StringToNumber, V8 `InternalStringToDouble`) and
//! `parseInt` (V8 `NumberParseIntHelper`, `src/numbers/conversions.cc`). This is the only place
//! of the workspace that may call `str::parse::<f64>` (it is correctly rounded like V8's
//! `Strtod`; Rust's own grammar accepts `inf`, `nan` and rejects spaces, so every input is
//! validated against the ECMA-262 grammar first).

use crate::fdlibm::scalbn;
use crate::text::{trim, trim_start};

/// The value of `c` as a digit of `radix` (`0-9`, `a-z`, `A-Z`), `None` when it is not one.
fn digit(c: u8, radix: u32) -> Option<u32> {
    let d = match c {
        b'0'..=b'9' => u32::from(c - b'0'),
        b'a'..=b'z' => u32::from(c - b'a') + 10,
        b'A'..=b'Z' => u32::from(c - b'A') + 10,
        _ => return None,
    };
    (d < radix).then_some(d)
}

/// `InternalStringToIntDouble<radix_log_2>(.., negative = false, allow_trailing_junk = true)`:
/// the digits at the start of `b` (radix 2, 4, 8, 16 or 32), rounded to a double half to even
/// when they exceed 53 bits. Reading stops at the first byte that is not a digit; junk after
/// the digits is the caller's business.
fn int_double(radix_log_2: u32, b: &[u8]) -> f64 {
    let radix = 1u32 << radix_log_2;
    let mut current = 0usize;
    while b.get(current) == Some(&b'0') {
        current += 1;
        if current == b.len() {
            return 0.0;
        }
    }

    let mut number: i64 = 0;
    let mut exponent: i32 = 0;
    while let Some(d) = b.get(current).and_then(|&c| digit(c, radix)) {
        number = number * i64::from(radix) + i64::from(d);
        let mut overflow = (number >> 53) as i32;
        if overflow != 0 {
            let mut overflow_bits_count = 1;
            while overflow > 1 {
                overflow_bits_count += 1;
                overflow >>= 1;
            }

            let dropped_bits_mask = (1i32 << overflow_bits_count) - 1;
            let dropped_bits = (number as i32) & dropped_bits_mask;
            number >>= overflow_bits_count;
            exponent = overflow_bits_count;

            let mut zero_tail = true;
            loop {
                current += 1;
                match b.get(current) {
                    Some(&c) if digit(c, radix).is_some() => {
                        zero_tail = zero_tail && c == b'0';
                        // V8's `int` wraps (radix 32 past 429,496,739 digits): the same here
                        exponent = exponent.wrapping_add(radix_log_2 as i32);
                    }
                    _ => break,
                }
            }

            let middle_value = 1i32 << (overflow_bits_count - 1);
            if dropped_bits > middle_value || (dropped_bits == middle_value && ((number & 1) != 0 || !zero_tail)) {
                number += 1;
            }

            if (number & (1i64 << 53)) != 0 {
                exponent = exponent.wrapping_add(1);
                number >>= 1;
            }
            break;
        }
        current += 1;
        if current == b.len() {
            break;
        }
    }

    if exponent == 0 {
        return number as f64;
    }
    scalbn(number as f64, exponent)
}

/// `Number(s)` and unary `+s` for a string: white space on both sides is ignored, the empty
/// string is 0, `0x`/`0o`/`0b` integers (no sign) are exact to 53 bits and rounded half to even
/// beyond, decimal literals with optional sign, fraction and exponent are correctly rounded,
/// `Infinity` is case sensitive, anything else (including `_`, a sign before `0x`, `1e`, `.`) is
/// NaN. Callers convert non-string operands themselves (`undefined` → NaN, `null` → 0, booleans).
#[must_use]
pub fn to_number(s: &str) -> f64 {
    let t = trim(s);
    let b = t.as_bytes();
    if b.is_empty() {
        return 0.0;
    }

    if let [b'0', prefix, rest @ ..] = b {
        let radix_log_2 = match prefix {
            b'x' | b'X' => 4,
            b'o' | b'O' => 3,
            b'b' | b'B' => 1,
            _ => 0,
        };
        if radix_log_2 != 0 {
            let radix = 1u32 << radix_log_2;
            if rest.is_empty() || !rest.iter().all(|&c| digit(c, radix).is_some()) {
                return f64::NAN;
            }
            return int_double(radix_log_2, rest);
        }
    }

    let (negative, unsigned) = match b {
        [b'-', rest @ ..] => (true, rest),
        [b'+', rest @ ..] => (false, rest),
        _ => (false, b),
    };
    if unsigned == b"Infinity" {
        return if negative { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    if !is_unsigned_decimal_literal(unsigned) {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// StrUnsignedDecimalLiteral without `Infinity`: `digits [. [digits]] [e [±] digits]` or
/// `. digits [e [±] digits]`.
fn is_unsigned_decimal_literal(b: &[u8]) -> bool {
    let count = |from: usize| b.get(from..).map_or(0, |r| r.iter().take_while(|c| c.is_ascii_digit()).count());
    let int_digits = count(0);
    let mut i = int_digits;
    let mut frac_digits = 0;
    if b.get(i) == Some(&b'.') {
        i += 1;
        frac_digits = count(i);
        i += frac_digits;
    }
    if int_digits + frac_digits == 0 {
        return false;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let exp_digits = count(i);
        if exp_digits == 0 {
            return false;
        }
        i += exp_digits;
    }
    i == b.len()
}

/// `parseInt(s, radix)`; `radix` is the argument after ToInt32 (0 when it was `undefined` or
/// 0). A radix outside 2..=36 is NaN. Leading white space and a sign are skipped, `0x` only
/// when the radix is 0 or 16, reading stops at the first character that is not a digit, `-0`
/// is −0, no digits is NaN. Radix 10 is correctly rounded, powers of two round half to even,
/// other radices accumulate in doubles exactly as V8 does.
#[must_use]
pub fn parse_int(s: &str, radix: i32) -> f64 {
    let b = trim_start(s).as_bytes();
    if b.is_empty() {
        return f64::NAN;
    }
    if radix != 0 && !(2..=36).contains(&radix) {
        return f64::NAN;
    }
    let mut radix = radix as u32;

    let mut current = 0usize;
    let mut negative = false;
    match b.first() {
        Some(b'+') => current = 1,
        Some(b'-') => {
            negative = true;
            current = 1;
        }
        _ => {}
    }
    if current == b.len() {
        return f64::NAN;
    }
    let signed_zero = if negative { -0.0 } else { 0.0 };

    let at = |i: usize| b.get(i).copied();
    let mut leading_zero = false;
    if radix == 0 {
        radix = 10;
        if at(current) == Some(b'0') {
            current += 1;
            if current == b.len() {
                return signed_zero;
            }
            if matches!(at(current), Some(b'x' | b'X')) {
                radix = 16;
                current += 1;
                if current == b.len() {
                    return f64::NAN;
                }
            } else {
                leading_zero = true;
            }
        }
    } else if radix == 16 && at(current) == Some(b'0') {
        current += 1;
        if current == b.len() {
            return signed_zero;
        }
        if matches!(at(current), Some(b'x' | b'X')) {
            current += 1;
            if current == b.len() {
                return f64::NAN;
            }
        } else {
            leading_zero = true;
        }
    }
    while at(current) == Some(b'0') {
        leading_zero = true;
        current += 1;
        if current == b.len() {
            return signed_zero;
        }
    }
    let first_is_digit = at(current).and_then(|c| digit(c, radix)).is_some();
    if leading_zero && !first_is_digit {
        return signed_zero;
    }
    if !leading_zero && !first_is_digit {
        return f64::NAN;
    }

    let digits = b.get(current..).unwrap_or(&[]);
    let result = if radix == 10 {
        parse_base_ten(digits)
    } else if radix.is_power_of_two() {
        int_double(radix.trailing_zeros(), digits)
    } else {
        parse_generic(digits, radix)
    };
    if negative { -result } else { result }
}

/// `HandleBaseTenCase`: the leading ASCII digits, as a decimal (V8 keeps 310 of them: more than
/// 309 digits is infinity, which the parse gives too).
fn parse_base_ten(digits: &[u8]) -> f64 {
    const MAX_SIGNIFICANT_DIGITS: usize = 309;
    let len = digits.iter().take_while(|c| c.is_ascii_digit()).count();
    if len > MAX_SIGNIFICANT_DIGITS + 1 {
        return f64::INFINITY;
    }
    let text = digits.get(..len).and_then(|d| core::str::from_utf8(d).ok()).unwrap_or("0");
    text.parse::<f64>().unwrap_or(f64::NAN)
}

/// `HandleGenericCase`: radix 3, 5, 6, 7, 9, 11…36 except 16 and 32. Parts of up to 32 bits
/// are accumulated as integers and combined in doubles, the (implementation dependent) rounding
/// error of the spec for large values is V8's.
fn parse_generic(digits: &[u8], radix: u32) -> f64 {
    const MAXIMUM_MULTIPLIER: u32 = 0xFFFF_FFFF / 36;
    let mut result = 0.0f64;
    let mut current = 0usize;
    let mut done = false;
    while !done {
        let mut part: u32 = 0;
        let mut multiplier: u32 = 1;
        loop {
            let Some(d) = digits.get(current).and_then(|&c| digit(c, radix)) else {
                done = true;
                break;
            };
            let m = multiplier * radix;
            if m > MAXIMUM_MULTIPLIER {
                break;
            }
            part = part * radix + d;
            multiplier = m;
            current += 1;
        }
        result = result * f64::from(multiplier) + f64::from(part);
    }
    result
}
