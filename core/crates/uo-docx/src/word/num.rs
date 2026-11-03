//! Number and string helpers of the JS the Word semantics use, beyond uo-jsnum: `toString(radix)`
//! for every number (V8's `DoubleToRadixCString`: fractions, values of 2^53 and more, NaN and the
//! infinities; uo-jsnum's `to_radix_string` covers safe integers only), `padStart`, and UTF-16
//! helpers for the TS string operations that index code units.

use uo_jsnum::conv::to_int32;
use uo_jsnum::format::to_js_string;
use uo_jsnum::math::floor;

const CHARS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// `Double(d).Exponent()` of V8 (the exponent of the 53-bit significand; denormals −1074).
fn exponent(d: f64) -> i32 {
    let biased = i32::try_from((d.to_bits() >> 52) & 0x7ff).unwrap_or(0);
    if biased == 0 { -1074 } else { biased - 1075 }
}

/// `Double(d).NextDouble()` of V8 for `d >= 0` finite.
fn next_double(d: f64) -> f64 {
    if d.is_infinite() { d } else { f64::from_bits(d.to_bits() + 1) }
}

fn digit_char(d: i32) -> u8 {
    usize::try_from(d).ok().and_then(|i| CHARS.get(i)).copied().unwrap_or(b'0')
}

/// `x.toString(radix)` (radix 2..=36) as V8 prints it (`DoubleToRadixCString`, conversions.cc).
#[must_use]
pub fn js_to_radix(x: f64, radix: u32) -> String {
    if radix == 10 {
        return to_js_string(x);
    }
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-Infinity".into() } else { "Infinity".into() };
    }
    let r = f64::from(radix);
    let ri = i32::try_from(radix).unwrap_or(16);
    let negative = x < 0.0;
    let value = if negative { -x } else { x };
    let mut integer = floor(value);
    let mut fraction = value - integer;
    let mut delta = 0.5 * (next_double(value) - value);
    delta = if delta > next_double(0.0) { delta } else { next_double(0.0) };
    let mut frac: Vec<u8> = Vec::new();
    if fraction >= delta {
        loop {
            fraction *= r;
            delta *= r;
            let digit = to_int32(fraction);
            frac.push(digit_char(digit));
            fraction -= f64::from(digit);
            if (fraction > 0.5 || (fraction == 0.5 && (digit & 1) != 0)) && fraction + delta > 1.0 {
                // carry over the digits already written
                loop {
                    let Some(c) = frac.pop() else {
                        integer += 1.0;
                        break;
                    };
                    let d = if c > b'9' { i32::from(c - b'a') + 10 } else { i32::from(c - b'0') };
                    if d + 1 < ri {
                        frac.push(digit_char(d + 1));
                        break;
                    }
                }
                break;
            }
            if fraction < delta {
                break;
            }
        }
    }
    let mut int: Vec<u8> = Vec::new();
    while exponent(integer / r) > 0 {
        integer /= r;
        int.push(b'0');
    }
    loop {
        let remainder = uo_jsnum::math::rem(integer, r);
        int.push(digit_char(to_int32(remainder)));
        integer = (integer - remainder) / r;
        if integer <= 0.0 {
            break;
        }
    }
    let mut out = String::with_capacity(int.len() + frac.len() + 2);
    if negative {
        out.push('-');
    }
    out.extend(int.iter().rev().map(|&b| char::from(b)));
    if !frac.is_empty() {
        out.push('.');
        out.extend(frac.iter().map(|&b| char::from(b)));
    }
    out
}

/// `s.padStart(w, '0')` for an ASCII `s`.
#[must_use]
pub fn pad_start_zero(s: &str, w: usize) -> String {
    let n = s.encode_utf16().count();
    if n >= w {
        return s.into();
    }
    let mut out = "0".repeat(w - n);
    out.push_str(s);
    out
}

/// `String(n).padStart(w, '0')` (`pad`, datefmt.ts:20).
#[must_use]
pub fn pad_num(n: f64, w: usize) -> String {
    pad_start_zero(&to_js_string(n), w)
}

/// The UTF-16 units of a Rust string.
#[must_use]
pub fn u16s(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// `s.slice(a, b)` over UTF-16 units with `0 <= a`, `b` clamped to the length.
#[must_use]
pub fn slice16(s: &[u16], a: usize, b: usize) -> &[u16] {
    let b = b.min(s.len());
    s.get(a.min(b)..b).unwrap_or(&[])
}

/// `parseInt(s.slice(a, b), radix)` over UTF-16 units.
#[must_use]
pub fn parse_int_slice(s: &[u16], a: usize, b: usize, radix: i32) -> f64 {
    uo_jsnum::parse::parse_int_utf16(slice16(s, a, b), radix)
}

/// `s.toUpperCase()` of a Rust string (no lone surrogates).
#[must_use]
pub fn upper(s: &str) -> String {
    uo_text::case::to_uppercase_str(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radix_like_v8() {
        // node -e "for (const x of [...]) console.log(x.toString(16))" (Node 24.15)
        let cases: [(f64, &str); 11] = [
            (255.0, "ff"),
            (1.5, "1.8"),
            (-0.0, "0"),
            (0.1, "0.1999999999999a"),
            (1_152_921_504_606_851_000.0, "1000000000001000"),
            (1e21, "3635c9adc5dea00000"),
            (-3.75, "-3.c"),
            (1.0 / 3.0, "0.55555555555554"),
            (15.999_999_999_99, "f.fffffffff5018"),
            (1e-7, "0.000001ad7f29abcaf48"),
            (123.456, "7b.74bc6a7ef9dc"),
        ];
        for (x, s) in cases {
            assert_eq!(js_to_radix(x, 16), s, "{x}");
        }
        assert_eq!(js_to_radix(f64::NAN, 16), "NaN");
        assert_eq!(js_to_radix(f64::NEG_INFINITY, 16), "-Infinity");
    }
}
