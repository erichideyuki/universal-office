//! `Number.prototype.toString(radix)` for integers: `DoubleToRadixStringView`
//! (`src/numbers/conversions.cc`) restricted to integral values below 2^53 (gate 1, G14: every
//! engine caller formats integers). For those the fraction part of V8's routine is empty (the
//! fraction is 0, below the delta) and the loop that fills unrepresented digits with zeros never
//! runs (`integer / radix` is far below 2^53), so what is left is V8's integer digit loop.

use super::RangeError;

/// 2^53: the integers below it are all exactly representable.
const TWO53: f64 = 9_007_199_254_740_992.0;

/// `x.toString(radix)` for an integral `x` with `|x| < 2^53` and `radix` in 2..=36 (lowercase
/// digits, `-` prefix, `0` for ±0). A fraction, `NaN`, an infinity, a value of 2^53 or more, or a
/// radix outside 2..=36 is a [`RangeError`]: the fraction digits of `toString(radix)` are not
/// ported (no engine caller needs them).
pub fn to_radix_string(x: f64, radix: u32) -> Result<String, RangeError> {
    if !(2..=36).contains(&radix) {
        return Err(RangeError);
    }
    if !x.is_finite() || x.trunc() != x || x.abs() >= TWO53 {
        return Err(RangeError);
    }
    // Zero is handled by the callers of V8's routine.
    if x == 0.0 {
        return Ok("0".to_owned());
    }

    // Character array used for conversion.
    const CHARS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    // 53 digits (radix 2) and the sign.
    let mut buffer = [0u8; 54];
    let mut integer_cursor = buffer.len();

    let negative = x < 0.0;
    let mut integer = x.abs();
    let radix = f64::from(radix);

    // Compute integer digits.
    loop {
        let remainder = integer % radix;
        integer_cursor -= 1;
        if let Some(slot) = buffer.get_mut(integer_cursor) {
            *slot = CHARS.get(remainder as usize).copied().unwrap_or(b'0');
        }
        integer = (integer - remainder) / radix;
        if integer <= 0.0 {
            break;
        }
    }

    // Add sign.
    if negative {
        integer_cursor -= 1;
        if let Some(slot) = buffer.get_mut(integer_cursor) {
            *slot = b'-';
        }
    }
    Ok(core::str::from_utf8(buffer.get(integer_cursor..).unwrap_or(&[])).unwrap_or("").to_owned())
}
