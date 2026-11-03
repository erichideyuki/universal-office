//! Number → string, as V8 13.6.233.17 does it: `Number::toString` (`Js`, `to_js_string`, on
//! `ryu-js`), `toFixed`, `toPrecision` and `toExponential` (V8's own digit routines, `crate::dtoa`,
//! and `DoubleTo{Fixed,Exponential,Precision}StringView` of `src/numbers/conversions.cc` here), and
//! `toString(radix)` for integers (`radix`).
//!
//! Each formatter allocates the returned `String` and nothing else (the digits and the text are
//! built on the stack); `Js` writes without allocating.

mod radix;

use core::fmt;

use crate::dtoa::{self, Mode};
pub use radix::to_radix_string;

/// The JS `RangeError` of an argument out of range (`toFixed(101)`, `toPrecision(0)`,
/// `toString(1)`...), or, for [`to_radix_string`], of a value outside what the function supports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeError;

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("argument out of range")
    }
}

impl core::error::Error for RangeError {}

/// `Number::toString(x)` (also `String(x)` and template literals): the shortest digits that read
/// back as `x`, with V8's choice between plain and exponent notation (`1e21`, `1.5e-7`), `-0` as
/// `0`, `NaN`, `Infinity`. Formats on the stack; `to_string()` allocates what the text needs.
#[derive(Clone, Copy, Debug)]
pub struct Js(pub f64);

impl fmt::Display for Js {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buffer = ryu_js::Buffer::new();
        f.write_str(buffer.format(self.0))
    }
}

/// `String(x)`: [`Js`] as a `String` (one allocation of exactly the text).
#[must_use]
pub fn to_js_string(x: f64) -> String {
    let mut buffer = ryu_js::Buffer::new();
    buffer.format(x).to_owned()
}

/// `kMaxFractionDigits` of V8.
const MAX_FRACTION_DIGITS: u32 = 100;
/// `kDoubleToFixedMaxDigitsBeforePoint` is 21: the first value `toFixed` leaves to `String(x)`.
const FIRST_NON_FIXED: f64 = 1e21;
/// Room for the longest result: `toFixed` of -(1e21 - 1) with 100 digits is 123 characters.
const TEXT_CAPACITY: usize = 192;

/// V8's `SimpleStringBuilder`, on the stack. Output beyond the capacity is dropped (no result
/// comes near it: see `TEXT_CAPACITY`).
struct Text {
    bytes: [u8; TEXT_CAPACITY],
    position: usize,
}

impl Text {
    fn new() -> Text {
        Text { bytes: [0; TEXT_CAPACITY], position: 0 }
    }

    fn add_character(&mut self, c: u8) {
        if let Some(slot) = self.bytes.get_mut(self.position) {
            *slot = c;
            self.position += 1;
        }
    }

    fn add_string(&mut self, s: &[u8]) {
        for &c in s {
            self.add_character(c);
        }
    }

    /// `AddPadding`: nothing for a count that is not positive.
    fn add_padding(&mut self, c: u8, count: i32) {
        for _ in 0..count {
            self.add_character(c);
        }
    }

    fn add_decimal_integer(&mut self, mut value: i32) {
        let mut digits = [0u8; 11];
        let mut i = digits.len();
        // Build the string backwards from the least significant digit.
        loop {
            i -= 1;
            if let Some(slot) = digits.get_mut(i) {
                *slot = b'0' + (value % 10) as u8;
            }
            value /= 10;
            if value == 0 {
                break;
            }
        }
        self.add_string(digits.get(i..).unwrap_or(&[]));
    }

    fn position(&self) -> i32 {
        self.position as i32
    }

    fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..self.position).unwrap_or(&[])
    }

    /// The text built (ASCII only).
    fn finish(&self) -> String {
        core::str::from_utf8(self.as_bytes()).unwrap_or("").to_owned()
    }
}

/// `substr[from..from + n]` of a byte slice, empty when out of range.
fn part(bytes: &[u8], from: i32, n: i32) -> &[u8] {
    let (Ok(from), Ok(n)) = (usize::try_from(from), usize::try_from(n)) else { return &[] };
    bytes.get(from..from.saturating_add(n)).unwrap_or(&[])
}

/// `NaN` and the infinities, which the three methods answer before looking at the digits.
fn non_finite(x: f64) -> Option<String> {
    if x.is_nan() {
        Some("NaN".to_owned())
    } else if x.is_infinite() {
        Some(if x < 0.0 { "-Infinity" } else { "Infinity" }.to_owned())
    } else {
        None
    }
}

/// `x.toFixed(digits)` (`NumberPrototypeToFixed` + `DoubleToFixedStringView`). `digits` outside
/// 0..=100 is a `RangeError`, checked before the value (as V8 does: `NaN.toFixed(101)` throws);
/// `|x| >= 1e21` is `String(x)`; a negative value that rounds to zero keeps its sign
/// (`(-1e-10).toFixed(2)` is `"-0.00"`), `-0` does not (`"0.00"`).
pub fn to_fixed(x: f64, digits: u32) -> Result<String, RangeError> {
    if digits > MAX_FRACTION_DIGITS {
        return Err(RangeError);
    }
    if let Some(text) = non_finite(x) {
        return Ok(text);
    }
    let f = digits as i32;

    let mut negative = false;
    let mut abs_value = x;
    if x < 0.0 {
        abs_value = -x;
        negative = true;
    }

    // If abs_value has more than kDoubleToFixedMaxDigitsBeforePoint digits before the point use the
    // non-fixed conversion routine.
    if abs_value >= FIRST_NON_FIXED {
        return Ok(to_js_string(x));
    }

    // Find a sufficiently precise decimal representation of n.
    let digits = dtoa::double_to_ascii(x, Mode::Fixed, f);
    let decimal_rep = digits.buffer.digits(digits.length);
    let decimal_rep_length = digits.length;
    let mut decimal_point = digits.point;

    // Create a representation that is padded with zeros if needed.
    let mut zero_prefix_length = 0;
    let mut zero_postfix_length = 0;

    if decimal_point <= 0 {
        zero_prefix_length = -decimal_point + 1;
        decimal_point = 1;
    }

    if zero_prefix_length + decimal_rep_length < decimal_point + f {
        zero_postfix_length = decimal_point + f - decimal_rep_length - zero_prefix_length;
    }

    let mut rep = Text::new();
    rep.add_padding(b'0', zero_prefix_length);
    rep.add_string(decimal_rep);
    rep.add_padding(b'0', zero_postfix_length);

    // Create the result string by appending a minus and putting in a decimal point if needed.
    let mut builder = Text::new();
    if negative {
        builder.add_character(b'-');
    }
    builder.add_string(part(rep.as_bytes(), 0, decimal_point));
    if f > 0 {
        builder.add_character(b'.');
        builder.add_string(part(rep.as_bytes(), decimal_point, f));
    }
    Ok(builder.finish())
}

/// `CreateExponentialRepresentation`.
fn create_exponential_representation(decimal_rep: &[u8], exponent: i32, negative: bool, significant_digits: i32, builder: &mut Text) {
    let mut exponent = exponent;
    let mut negative_exponent = false;
    if exponent < 0 {
        negative_exponent = true;
        exponent = -exponent;
    }

    if negative {
        builder.add_character(b'-');
    }
    builder.add_character(decimal_rep.first().copied().unwrap_or(b'0'));
    if significant_digits != 1 {
        builder.add_character(b'.');
        builder.add_string(decimal_rep.get(1..).unwrap_or(&[]));
        builder.add_padding(b'0', significant_digits - decimal_rep.len() as i32);
    }

    builder.add_character(b'e');
    builder.add_character(if negative_exponent { b'-' } else { b'+' });
    builder.add_decimal_integer(exponent);
}

/// `x.toExponential(digits)` with the digits given (`DoubleToExponentialStringView`; the form without
/// an argument, which prints the shortest digits, is not provided). `NaN` and the infinities are
/// answered before `digits` is checked (V8: `NaN.toExponential(200)` is `"NaN"`); `digits` outside
/// 0..=100 is a `RangeError`.
pub fn to_exponential(x: f64, digits: u32) -> Result<String, RangeError> {
    if let Some(text) = non_finite(x) {
        return Ok(text);
    }
    if digits > MAX_FRACTION_DIGITS {
        return Err(RangeError);
    }
    let f = digits as i32;

    let mut negative = false;
    let mut value = x;
    if value < 0.0 {
        value = -value;
        negative = true;
    }

    // Find a sufficiently precise decimal representation of n. f corresponds to the digits after
    // the point. There is always one digit before the point. The number of requested_digits equals
    // hence f + 1.
    let digits = dtoa::double_to_ascii(value, Mode::Precision, f + 1);
    let decimal_rep = digits.buffer.digits(digits.length);

    let exponent = digits.point - 1;
    let mut builder = Text::new();
    create_exponential_representation(decimal_rep, exponent, negative, f + 1, &mut builder);
    Ok(builder.finish())
}

/// `x.toPrecision(precision)` (`DoubleToPrecisionStringView`). `NaN` and the infinities are
/// answered before `precision` is checked; `precision` outside 1..=100 is a `RangeError`. (The form
/// without an argument is `String(x)`: [`to_js_string`].)
pub fn to_precision(x: f64, precision: u32) -> Result<String, RangeError> {
    if let Some(text) = non_finite(x) {
        return Ok(text);
    }
    if !(1..=MAX_FRACTION_DIGITS).contains(&precision) {
        return Err(RangeError);
    }
    let p = precision as i32;

    let mut negative = false;
    let mut value = x;
    if value < 0.0 {
        value = -value;
        negative = true;
    }

    // Find a sufficiently precise decimal representation of n.
    let digits = dtoa::double_to_ascii(value, Mode::Precision, p);
    let decimal_rep = digits.buffer.digits(digits.length);
    let decimal_rep_length = digits.length;
    let decimal_point = digits.point;

    let exponent = decimal_point - 1;

    let mut builder = Text::new();
    if exponent < -6 || exponent >= p {
        create_exponential_representation(decimal_rep, exponent, negative, p, &mut builder);
    } else {
        // Use fixed notation.
        if negative {
            builder.add_character(b'-');
        }
        if decimal_point <= 0 {
            builder.add_string(b"0.");
            builder.add_padding(b'0', -decimal_point);
            builder.add_string(decimal_rep);
            builder.add_padding(b'0', p - decimal_rep_length);
        } else {
            let m = decimal_rep_length.min(decimal_point);
            builder.add_string(part(decimal_rep, 0, m));
            builder.add_padding(b'0', decimal_point - decimal_rep_length);
            if decimal_point < p {
                builder.add_character(b'.');
                let extra = if negative { 2 } else { 1 };
                if decimal_rep_length > decimal_point {
                    let len = decimal_rep_length - decimal_point;
                    let n = len.min(p - (builder.position() - extra));
                    builder.add_string(part(decimal_rep, decimal_point, n));
                }
                builder.add_padding(b'0', extra + (p - builder.position()));
            }
        }
    }
    Ok(builder.finish())
}
