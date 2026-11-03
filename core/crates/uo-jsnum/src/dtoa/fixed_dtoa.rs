//! `FastFixedDtoa` of `src/base/numbers/fixed-dtoa.cc`: the digits after the decimal point of a
//! double, exactly, for values below 2^73 and at most 20 fraction digits (`false` otherwise: the
//! caller falls back to the bignum algorithm).

use super::Buffer;
use super::double::Double;

/// Represents a 128bit type (V8 replaces it by a native type on platforms that have one).
struct UInt128 {
    high_bits: u64,
    low_bits: u64,
}

const MASK32: u64 = 0xFFFF_FFFF;

impl UInt128 {
    fn new(high: u64, low: u64) -> UInt128 {
        UInt128 { high_bits: high, low_bits: low }
    }

    fn multiply(&mut self, multiplicand: u32) {
        let multiplicand = u64::from(multiplicand);
        let mut accumulator = (self.low_bits & MASK32).wrapping_mul(multiplicand);
        let mut part = (accumulator & MASK32) as u32;
        accumulator >>= 32;
        accumulator = accumulator.wrapping_add((self.low_bits >> 32).wrapping_mul(multiplicand));
        self.low_bits = (accumulator << 32).wrapping_add(u64::from(part));
        accumulator >>= 32;
        accumulator = accumulator.wrapping_add((self.high_bits & MASK32).wrapping_mul(multiplicand));
        part = (accumulator & MASK32) as u32;
        accumulator >>= 32;
        accumulator = accumulator.wrapping_add((self.high_bits >> 32).wrapping_mul(multiplicand));
        self.high_bits = (accumulator << 32).wrapping_add(u64::from(part));
    }

    /// Shift in `[-64, 64]`: negative shifts to the left.
    fn shift(&mut self, shift_amount: i32) {
        if shift_amount == 0 {
            return;
        }
        if shift_amount == -64 {
            self.high_bits = self.low_bits;
            self.low_bits = 0;
        } else if shift_amount == 64 {
            self.low_bits = self.high_bits;
            self.high_bits = 0;
        } else if shift_amount <= 0 {
            self.high_bits <<= -shift_amount;
            self.high_bits = self.high_bits.wrapping_add(self.low_bits >> (64 + shift_amount));
            self.low_bits <<= -shift_amount;
        } else {
            self.low_bits >>= shift_amount;
            self.low_bits = self.low_bits.wrapping_add(self.high_bits << (64 - shift_amount));
            self.high_bits >>= shift_amount;
        }
    }

    /// Modifies *this to *this MOD (2^power). Returns *this DIV (2^power).
    fn div_mod_power_of_2(&mut self, power: i32) -> i32 {
        if power >= 64 {
            let result = (self.high_bits >> (power - 64)) as i32;
            self.high_bits = self.high_bits.wrapping_sub((result as u64) << (power - 64));
            result
        } else {
            let part_low = self.low_bits >> power;
            let part_high = self.high_bits << (64 - power);
            let result = part_low.wrapping_add(part_high) as i32;
            self.high_bits = 0;
            self.low_bits = self.low_bits.wrapping_sub(part_low << power);
            result
        }
    }

    fn is_zero(&self) -> bool {
        self.high_bits == 0 && self.low_bits == 0
    }

    fn bit_at(&self, position: i32) -> i32 {
        if position >= 64 { ((self.high_bits >> (position - 64)) as i32) & 1 } else { ((self.low_bits >> position) as i32) & 1 }
    }
}

/// Includes the hidden bit.
const DOUBLE_SIGNIFICAND_SIZE: i32 = 53;

fn fill_digits32_fixed_length(mut number: u32, requested_length: i32, buffer: &mut Buffer, length: &mut i32) {
    let mut i = requested_length - 1;
    while i >= 0 {
        buffer.set(*length + i, b'0' + (number % 10) as u8);
        number /= 10;
        i -= 1;
    }
    *length += requested_length;
}

fn fill_digits32(mut number: u32, buffer: &mut Buffer, length: &mut i32) {
    let mut number_length = 0;
    // We fill the digits in reverse order and exchange them afterwards.
    while number != 0 {
        let digit = number % 10;
        number /= 10;
        buffer.set(*length + number_length, b'0' + digit as u8);
        number_length += 1;
    }
    // Exchange the digits.
    let mut i = *length;
    let mut j = *length + number_length - 1;
    while i < j {
        let tmp = buffer.get(i);
        buffer.set(i, buffer.get(j));
        buffer.set(j, tmp);
        i += 1;
        j -= 1;
    }
    *length += number_length;
}

fn fill_digits64_fixed_length(mut number: u64, _requested_length: i32, buffer: &mut Buffer, length: &mut i32) {
    const TEN7: u32 = 10_000_000;
    // For efficiency cut the number into 3 uint32_t parts, and print those.
    let part2 = (number % u64::from(TEN7)) as u32;
    number /= u64::from(TEN7);
    let part1 = (number % u64::from(TEN7)) as u32;
    let part0 = (number / u64::from(TEN7)) as u32;

    fill_digits32_fixed_length(part0, 3, buffer, length);
    fill_digits32_fixed_length(part1, 7, buffer, length);
    fill_digits32_fixed_length(part2, 7, buffer, length);
}

fn fill_digits64(mut number: u64, buffer: &mut Buffer, length: &mut i32) {
    const TEN7: u32 = 10_000_000;
    // For efficiency cut the number into 3 uint32_t parts, and print those.
    let part2 = (number % u64::from(TEN7)) as u32;
    number /= u64::from(TEN7);
    let part1 = (number % u64::from(TEN7)) as u32;
    let part0 = (number / u64::from(TEN7)) as u32;

    if part0 != 0 {
        fill_digits32(part0, buffer, length);
        fill_digits32_fixed_length(part1, 7, buffer, length);
        fill_digits32_fixed_length(part2, 7, buffer, length);
    } else if part1 != 0 {
        fill_digits32(part1, buffer, length);
        fill_digits32_fixed_length(part2, 7, buffer, length);
    } else {
        fill_digits32(part2, buffer, length);
    }
}

fn dtoa_round_up(buffer: &mut Buffer, length: &mut i32, decimal_point: &mut i32) {
    // An empty buffer represents 0.
    if *length == 0 {
        buffer.set(0, b'1');
        *decimal_point = 1;
        *length = 1;
        return;
    }
    // Round the last digit until we either have a digit that was not '9' or until we reached the
    // first digit.
    buffer.increment(*length - 1);
    let mut i = *length - 1;
    while i > 0 {
        if buffer.get(i) != b'0' + 10 {
            return;
        }
        buffer.set(i, b'0');
        buffer.increment(i - 1);
        i -= 1;
    }
    // If the first digit is now '0' + 10, we would need to set it to '0' and add a '1' in front.
    // However we reach the first digit only if all following digits had been '9' before rounding
    // up. Now all trailing digits are '0' and we simply switch the first digit to '1' and update
    // the decimal-point (indicating that the point is now one digit to the right).
    if buffer.get(0) == b'0' + 10 {
        buffer.set(0, b'1');
        *decimal_point += 1;
    }
}

/// The given fractionals number represents a fixed-point number with binary point at bit
/// (-exponent). Preconditions: `-128 <= exponent <= 0`, `0 <= fractionals * 2^exponent < 1`. The
/// function will round its result; during the rounding-process digits not generated by this
/// function might be updated, and the decimal-point variable might be updated.
fn fill_fractionals(
    mut fractionals: u64,
    exponent: i32,
    fractional_count: i32,
    buffer: &mut Buffer,
    length: &mut i32,
    decimal_point: &mut i32,
) {
    // 'fractionals' is a fixed-point number, with binary point at bit (-exponent). Inside the
    // function the non-converted remainder of fractionals is a fixed-point number, with binary
    // point at bit 'point'.
    if -exponent <= 64 {
        // One 64 bit number is sufficient.
        let mut point = -exponent;
        let mut i = 0;
        while i < fractional_count {
            if fractionals == 0 {
                break;
            }
            // Instead of multiplying by 10 we multiply by 5 and adjust the point location. This way
            // the fractionals variable will not overflow. Invariant at the beginning of the loop:
            // fractionals < 2^point. Initially we have: point <= 64 and fractionals < 2^56. After
            // each iteration the point is decremented by one. Note that 5^3 = 125 < 128 = 2^7.
            // Therefore three iterations of this loop will not overflow fractionals (even without
            // the subtraction at the end of the loop body). At this time point will satisfy
            // point <= 61 and therefore fractionals < 2^point and any further multiplication of
            // fractionals by 5 will not overflow.
            fractionals = fractionals.wrapping_mul(5);
            point -= 1;
            let digit = (fractionals >> point) as i32;
            buffer.set(*length, b'0' + digit as u8);
            *length += 1;
            fractionals = fractionals.wrapping_sub((digit as u64) << point);
            i += 1;
        }
        // If the first bit after the point is set we have to round up.
        if point > 0 && ((fractionals >> (point - 1)) & 1) == 1 {
            dtoa_round_up(buffer, length, decimal_point);
        }
    } else {
        // We need 128 bits.
        let mut fractionals128 = UInt128::new(fractionals, 0);
        fractionals128.shift(-exponent - 64);
        let mut point = 128;
        let mut i = 0;
        while i < fractional_count {
            if fractionals128.is_zero() {
                break;
            }
            // As before: instead of multiplying by 10 we multiply by 5 and adjust the point
            // location. This multiplication will not overflow for the same reasons as before.
            fractionals128.multiply(5);
            point -= 1;
            let digit = fractionals128.div_mod_power_of_2(point);
            buffer.set(*length, b'0' + digit as u8);
            *length += 1;
            i += 1;
        }
        if fractionals128.bit_at(point - 1) == 1 {
            dtoa_round_up(buffer, length, decimal_point);
        }
    }
}

/// Removes leading and trailing zeros. If leading zeros are removed then the decimal point
/// position is adjusted.
fn trim_zeros(buffer: &mut Buffer, length: &mut i32, decimal_point: &mut i32) {
    while *length > 0 && buffer.get(*length - 1) == b'0' {
        *length -= 1;
    }
    let mut first_non_zero = 0;
    while first_non_zero < *length && buffer.get(first_non_zero) == b'0' {
        first_non_zero += 1;
    }
    if first_non_zero != 0 {
        let mut i = first_non_zero;
        while i < *length {
            buffer.set(i - first_non_zero, buffer.get(i));
            i += 1;
        }
        *length -= first_non_zero;
        *decimal_point -= first_non_zero;
    }
}

/// `FastFixedDtoa(v, fractional_count, buffer, &length, &decimal_point)`: `v` is positive and finite.
pub(super) fn fast_fixed_dtoa(v: f64, fractional_count: i32, buffer: &mut Buffer, length: &mut i32, decimal_point: &mut i32) -> bool {
    const MAX_UINT32: u32 = 0xFFFF_FFFF;
    let mut significand = Double::new(v).significand();
    let exponent = Double::new(v).exponent();
    // v = significand * 2^exponent (with significand a 53bit integer). If the exponent is larger
    // than 20 (i.e. we may have a 73bit number) then we don't know how to compute the
    // representation. 2^73 ~= 9.5*10^21. If necessary this limit could probably be increased, but
    // we don't need more.
    if exponent > 20 {
        return false;
    }
    if fractional_count > 20 {
        return false;
    }
    *length = 0;
    // At most kDoubleSignificandSize bits of the significand are non-zero. Given a 64 bit integer
    // we have 11 0s followed by 53 potentially non-zero bits:  0..11*..0xxx..53*..xx
    if exponent + DOUBLE_SIGNIFICAND_SIZE > 64 {
        // The exponent must be > 11.
        //
        // We know that v = significand * 2^exponent. And the exponent > 11. We simplify the task by
        // dividing v by 10^17. The quotient delivers the first digits, and the remainder fits into
        // a 64 bit number. Dividing by 10^17 is equivalent to dividing by 5^17*2^17.
        const FIVE17: u64 = 0xB1_A2BC_2EC5; // 5^17
        let mut divisor = FIVE17;
        let divisor_power = 17;
        let mut dividend = significand;
        let quotient: u32;
        let remainder: u64;
        // Let v = f * 2^e with f == significand and e == exponent. Then need q (quotient) and r
        // (remainder) as follows:
        //   v            = q * 10^17       + r
        //   f * 2^e      = q * 10^17       + r
        //   f * 2^e      = q * 5^17 * 2^17 + r
        // If e > 17 then
        //   f * 2^(e-17) = q * 5^17        + r/2^17
        // else
        //   f  = q * 5^17 * 2^(17-e) + r/2^e
        if exponent > divisor_power {
            // We only allow exponents of up to 20 and therefore (17 - e) <= 3
            dividend <<= exponent - divisor_power;
            quotient = (dividend / divisor) as u32;
            remainder = (dividend % divisor) << divisor_power;
        } else {
            divisor <<= divisor_power - exponent;
            quotient = (dividend / divisor) as u32;
            remainder = (dividend % divisor) << exponent;
        }
        fill_digits32(quotient, buffer, length);
        fill_digits64_fixed_length(remainder, divisor_power, buffer, length);
        *decimal_point = *length;
    } else if exponent >= 0 {
        // 0 <= exponent <= 11
        significand <<= exponent;
        fill_digits64(significand, buffer, length);
        *decimal_point = *length;
    } else if exponent > -DOUBLE_SIGNIFICAND_SIZE {
        // We have to cut the number.
        let integrals = significand >> -exponent;
        let fractionals = significand - (integrals << -exponent);
        if integrals > u64::from(MAX_UINT32) {
            fill_digits64(integrals, buffer, length);
        } else {
            fill_digits32(integrals as u32, buffer, length);
        }
        *decimal_point = *length;
        fill_fractionals(fractionals, exponent, fractional_count, buffer, length, decimal_point);
    } else if exponent < -128 {
        // This configuration (with at most 20 digits) means that all digits must be 0.
        buffer.set(0, 0);
        *length = 0;
        *decimal_point = -fractional_count;
    } else {
        *decimal_point = 0;
        fill_fractionals(significand, exponent, fractional_count, buffer, length, decimal_point);
    }
    trim_zeros(buffer, length, decimal_point);
    buffer.set(*length, 0);
    if *length == 0 {
        // The string is empty and the decimal_point thus has no importance. Mimick Gay's dtoa and
        // and set it to -fractional_count.
        *decimal_point = -fractional_count;
    }
    true
}
