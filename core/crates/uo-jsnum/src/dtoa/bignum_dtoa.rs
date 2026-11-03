//! `BignumDtoa` of `src/base/numbers/bignum-dtoa.cc`: the exact algorithm, used when the fast one
//! of the mode cannot prove its result. Modes fixed and precision only: the shortest mode (and with
//! it `GenerateShortestDigits` and the boundary deltas, which only it fills) is `ryu-js`. The
//! boundary deltas stay in `fixup_multiply10` as the zero bignums V8 passes when
//! `need_boundary_deltas` is false, so the statement is the one of V8.

use super::Buffer;
use super::bignum::Bignum;
use super::double::{Double, HIDDEN_BIT, SIGNIFICAND_SIZE};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    /// `BIGNUM_DTOA_FIXED`
    Fixed,
    /// `BIGNUM_DTOA_PRECISION`
    Precision,
}

fn normalized_exponent(mut significand: u64, mut exponent: i32) -> i32 {
    while (significand & HIDDEN_BIT) == 0 {
        significand <<= 1;
        exponent -= 1;
    }
    exponent
}

/// `BignumDtoa(v, mode, requested_digits, buffer, &length, &decimal_point)`: `v` is positive and finite.
pub(super) fn bignum_dtoa(v: f64, mode: Mode, requested_digits: i32, buffer: &mut Buffer, length: &mut i32, decimal_point: &mut i32) {
    let significand = Double::new(v).significand();
    let is_even = (significand & 1) == 0;
    let exponent = Double::new(v).exponent();
    let normalized_exponent = normalized_exponent(significand, exponent);
    // estimated_power might be too low by 1.
    let estimated_power = estimate_power(normalized_exponent);

    // Shortcut for Fixed. The requested digits correspond to the digits after the point. If the
    // number is much too small, then there is no need in trying to get any digits.
    if mode == Mode::Fixed && -estimated_power - 1 > requested_digits {
        buffer.set(0, 0);
        *length = 0;
        // Set decimal-point to -requested_digits. This is what Gay does. Note that it should not
        // have any effect anyways since the string is empty.
        *decimal_point = -requested_digits;
        return;
    }

    let mut numerator = Bignum::new();
    let mut denominator = Bignum::new();
    let mut delta_minus = Bignum::new();
    let mut delta_plus = Bignum::new();
    // Make sure the bignum can grow large enough. The smallest double equals 4e-324. In this case
    // the denominator needs fewer than 324*4 binary digits. The maximum double is
    // 1.7976931348623157e308 which needs fewer than 308*4 binary digits.
    initial_scaled_start_values(v, estimated_power, &mut numerator, &mut denominator);
    // We now have v = (numerator / denominator) * 10^estimated_power.
    fixup_multiply10(estimated_power, is_even, decimal_point, &mut numerator, &mut denominator, &mut delta_minus, &mut delta_plus);
    // We now have v = (numerator / denominator) * 10^(decimal_point-1), and
    //  1 <= (numerator + delta_plus) / denominator < 10
    match mode {
        Mode::Fixed => bignum_to_fixed(requested_digits, decimal_point, &mut numerator, &mut denominator, buffer, length),
        Mode::Precision => generate_counted_digits(requested_digits, decimal_point, &mut numerator, &denominator, buffer, length),
    }
    buffer.set(*length, 0);
}

/// Let v = numerator / denominator < 10. Then we generate 'count' digits of d = x.xxxxx...
/// (without the decimal point) from left to right. Once 'count' digits have been produced we
/// decide wether to round up or down. Remainders of exactly .5 round upwards. Numbers such as
/// 9.999999 propagate a carry all the way, and change the exponent (decimal_point), when rounding
/// upwards.
fn generate_counted_digits(
    count: i32,
    decimal_point: &mut i32,
    numerator: &mut Bignum,
    denominator: &Bignum,
    buffer: &mut Buffer,
    length: &mut i32,
) {
    let mut i = 0;
    while i < count - 1 {
        let digit = numerator.divide_modulo_int_bignum(denominator);
        // digit = numerator / denominator (integer division).
        // numerator = numerator % denominator.
        buffer.set(i, (digit as u8).wrapping_add(b'0'));
        // Prepare for next iteration.
        numerator.times10();
        i += 1;
    }
    // Generate the last digit.
    let mut digit = numerator.divide_modulo_int_bignum(denominator);
    if Bignum::plus_compare(numerator, numerator, denominator) >= 0 {
        digit = digit.wrapping_add(1);
    }
    buffer.set(count - 1, (digit as u8).wrapping_add(b'0'));
    // Correct bad digits (in case we had a sequence of '9's). Propagate the carry until we hat a
    // non-'9' or til we reach the first digit.
    let mut i = count - 1;
    while i > 0 {
        if buffer.get(i) != b'0' + 10 {
            break;
        }
        buffer.set(i, b'0');
        buffer.increment(i - 1);
        i -= 1;
    }
    if buffer.get(0) == b'0' + 10 {
        // Propagate a carry past the top place.
        buffer.set(0, b'1');
        *decimal_point += 1;
    }
    *length = count;
}

/// Generates 'requested_digits' after the decimal point. It might omit trailing '0's. If the input
/// number is too small then no digits at all are generated (ex.: 2 fixed digits for 0.00001).
///
/// Input verifies:  1 <= (numerator + delta) / denominator < 10.
fn bignum_to_fixed(
    requested_digits: i32,
    decimal_point: &mut i32,
    numerator: &mut Bignum,
    denominator: &mut Bignum,
    buffer: &mut Buffer,
    length: &mut i32,
) {
    // Note that we have to look at more than just the requested_digits, since a number could be
    // rounded up. Example: v=0.5 with requested_digits=0. Even though the power of v equals 0 we
    // can't just stop here.
    if -(*decimal_point) > requested_digits {
        // The number is definitively too small. Ex: 0.001 with requested_digits == 1. Set
        // decimal-point to -requested_digits. This is what Gay does. Note that it should not have
        // any effect anyways since the string is empty.
        *decimal_point = -requested_digits;
        *length = 0;
    } else if -(*decimal_point) == requested_digits {
        // We only need to verify if the number rounds down or up. Ex: 0.04 and 0.06 with
        // requested_digits == 1.
        // Initially the fraction lies in range (1, 10]. Multiply the denominator by 10 so that we
        // can compare more easily.
        denominator.times10();
        if Bignum::plus_compare(numerator, numerator, denominator) >= 0 {
            // If the fraction is >= 0.5 then we have to include the rounded digit.
            buffer.set(0, b'1');
            *length = 1;
            *decimal_point += 1;
        } else {
            // Note that we caught most of similar cases earlier.
            *length = 0;
        }
    } else {
        // The requested digits correspond to the digits after the point. The variable
        // 'needed_digits' includes the digits before the point.
        let needed_digits = (*decimal_point) + requested_digits;
        generate_counted_digits(needed_digits, decimal_point, numerator, denominator, buffer, length);
    }
}

/// Returns an estimation of k such that 10^(k-1) <= v < 10^k where v = f * 2^exponent and
/// 2^52 <= f < 2^53. v is hence a normalized double with the given exponent. The output is an
/// approximation for the exponent of the decimal approimation .digits * 10^k.
///
/// The result might undershoot by 1 in which case 10^k <= v < 10^k+1.
///
/// Examples: EstimatePower(0) => 16, EstimatePower(-52) => 0.
fn estimate_power(exponent: i32) -> i32 {
    // This function estimates log10 of v where v = f*2^e (with e == exponent). Given that
    // log10(v) == log2(v)/log2(10) and e+(len(f)-1) is quite close to log2(v) the function is
    // simplified to (e+(len(f)-1)/log2(10)). Since we want to avoid overshooting we decrement by
    // 1e10 so that floating-point imprecisions don't affect us.
    const K1_LOG10: f64 = 0.30102999566398114; // 1/lg(10)

    // For doubles len(f) == 53 (don't forget the hidden bit).
    let estimate = (f64::from(exponent + SIGNIFICAND_SIZE - 1) * K1_LOG10 - 1e-10).ceil();
    estimate as i32
}

/// See comments for `initial_scaled_start_values`.
fn initial_scaled_start_values_positive_exponent(v: f64, estimated_power: i32, numerator: &mut Bignum, denominator: &mut Bignum) {
    // A positive exponent implies a positive power. Since the estimated_power is positive we simply
    // multiply the denominator by 10^estimated_power.

    // numerator = v.
    numerator.assign_uint64(Double::new(v).significand());
    numerator.shift_left(Double::new(v).exponent());
    // denominator = 10^estimated_power.
    denominator.assign_power_uint16(10, estimated_power);
}

/// See comments for `initial_scaled_start_values`.
fn initial_scaled_start_values_negative_exponent_positive_power(
    v: f64,
    estimated_power: i32,
    numerator: &mut Bignum,
    denominator: &mut Bignum,
) {
    let significand = Double::new(v).significand();
    let exponent = Double::new(v).exponent();
    // v = f * 2^e with e < 0, and with estimated_power >= 0. This means that e is close to 0 (have
    // a look at how estimated_power is computed).

    // numerator = significand
    //  since v = significand * 2^exponent this is equivalent to
    //  numerator = v * / 2^-exponent
    numerator.assign_uint64(significand);
    // denominator = 10^estimated_power * 2^-exponent (with exponent < 0)
    denominator.assign_power_uint16(10, estimated_power);
    denominator.shift_left(-exponent);
}

/// See comments for `initial_scaled_start_values`.
fn initial_scaled_start_values_negative_exponent_negative_power(
    v: f64,
    estimated_power: i32,
    numerator: &mut Bignum,
    denominator: &mut Bignum,
) {
    let significand = Double::new(v).significand();
    let exponent = Double::new(v).exponent();
    // Instead of multiplying the denominator with 10^estimated_power we multiply all values
    // (numerator and deltas) by 10^-estimated_power.

    // Use numerator as temporary container for power_ten.
    numerator.assign_power_uint16(10, -estimated_power);

    // numerator = significand * 2 * 10^-estimated_power
    //  since v = significand * 2^exponent this is equivalent to
    // numerator = v * 10^-estimated_power * 2 * 2^-exponent.
    // Remember: numerator has been abused as power_ten. So no need to assign it to itself.
    numerator.multiply_by_uint64(significand);

    // denominator = 2 * 2^-exponent with exponent < 0.
    denominator.assign_uint16(1);
    denominator.shift_left(-exponent);
}

/// Let v = significand * 2^exponent. Computes v / 10^estimated_power exactly, as a ratio of two
/// bignums, numerator and denominator. The functions `generate_counted_digits` and
/// `bignum_to_fixed` will then convert this ratio to its decimal representation d, with the
/// required accuracy. Then d * 10^estimated_power is the representation of v. (Note: the fraction
/// and the estimated_power might get adjusted before generating the decimal representation.)
///
/// Since 10^(k-1) <= v < 10^k (with k == estimated_power) or 10^k <= v < 10^(k+1) we then have
/// 0.1 <= numerator/denominator < 1 or 1 <= numerator/denominator < 10.
fn initial_scaled_start_values(v: f64, estimated_power: i32, numerator: &mut Bignum, denominator: &mut Bignum) {
    if Double::new(v).exponent() >= 0 {
        initial_scaled_start_values_positive_exponent(v, estimated_power, numerator, denominator);
    } else if estimated_power >= 0 {
        initial_scaled_start_values_negative_exponent_positive_power(v, estimated_power, numerator, denominator);
    } else {
        initial_scaled_start_values_negative_exponent_negative_power(v, estimated_power, numerator, denominator);
    }
}

/// This routine multiplies numerator/denominator so that its values lies in the range 1-10. That is
/// after a call to this function we have: 1 <= (numerator + delta_plus) /denominator < 10. Let
/// numerator the input before modification and numerator' the argument after modification, then the
/// output-parameter decimal_point is such that
///   numerator / denominator * 10^estimated_power == numerator' / denominator' * 10^(decimal_point - 1)
/// In some cases estimated_power was too low, and this is already the case. We then simply adjust
/// the power so that 10^(k-1) <= v < 10^k (with k == estimated_power) but do not touch the numerator
/// or denominator. Otherwise the routine multiplies the numerator and the deltas by 10.
fn fixup_multiply10(
    estimated_power: i32,
    is_even: bool,
    decimal_point: &mut i32,
    numerator: &mut Bignum,
    denominator: &mut Bignum,
    delta_minus: &mut Bignum,
    delta_plus: &mut Bignum,
) {
    let in_range = if is_even {
        // For IEEE doubles half-way cases (in decimal system numbers ending with 5) are rounded to
        // the closest floating-point number with even significand.
        Bignum::plus_compare(numerator, delta_plus, denominator) >= 0
    } else {
        Bignum::plus_compare(numerator, delta_plus, denominator) > 0
    };
    if in_range {
        // Since numerator + delta_plus >= denominator we already have 1 <= numerator/denominator <
        // 10. Simply update the estimated_power.
        *decimal_point = estimated_power + 1;
    } else {
        *decimal_point = estimated_power;
        numerator.times10();
        if Bignum::equal(delta_minus, delta_plus) {
            delta_minus.times10();
            delta_plus.assign_bignum(delta_minus);
        } else {
            delta_minus.times10();
            delta_plus.times10();
        }
    }
}
