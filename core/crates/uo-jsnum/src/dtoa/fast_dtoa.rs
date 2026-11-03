//! `FastDtoa` of `src/base/numbers/fast-dtoa.cc` in `FAST_DTOA_PRECISION` mode: Grisu3 "counted",
//! which generates a requested number of digits of a double and gives up (`false`) when its 64-bit
//! arithmetic cannot decide the rounding. The shortest mode (`Grisu3`, `DigitGen`, `RoundWeed`) is
//! `ryu-js`, see `crate::format`.

use super::Buffer;
use super::cached_powers::get_cached_power_for_binary_exponent_range;
use super::diy_fp::DiyFp;
use super::double::Double;

// The minimal and maximal target exponent define the range of w's binary exponent, where 'w' is
// the result of multiplying the input by a cached power of ten.
//
// A different range might be chosen on a different platform, to optimize digit generation, but a
// smaller range requires more powers of ten to be cached.
const MINIMAL_TARGET_EXPONENT: i32 = -60;
const MAXIMAL_TARGET_EXPONENT: i32 = -32;

/// Rounds the buffer upwards if the result is closer to v by possibly adding 1 to the buffer. If
/// the precision of the calculation is not sufficient to round correctly, return false. The
/// rounding might shift the whole buffer in which case the kappa is adjusted. For example "99",
/// kappa = 3 might become "10", kappa = 4.
///
/// If 2*rest > ten_kappa then the buffer needs to be round up. rest can have an error of +/- 1
/// unit. This function accounts for the imprecision and returns false, if the rounding direction
/// cannot be unambiguously determined.
///
/// Precondition: rest < ten_kappa.
fn round_weed_counted(buffer: &mut Buffer, length: i32, rest: u64, ten_kappa: u64, unit: u64, kappa: &mut i32) -> bool {
    // The following tests are done in a specific order to avoid overflows. They will work correctly
    // with any uint64 values of rest < ten_kappa and unit.
    //
    // If the unit is too big, then we don't know which way to round. For example a unit of 50 means
    // that the real number lies within rest +/- 50. If 10^kappa == 40 then there is no way to tell
    // which way to round.
    if unit >= ten_kappa {
        return false;
    }
    // Even if unit is just half the size of 10^kappa we are already completely lost. (And after the
    // previous test we know that the expression will not over/underflow.)
    if ten_kappa - unit <= unit {
        return false;
    }
    // If 2 * (rest + unit) <= 10^kappa we can safely round down.
    if (ten_kappa - rest > rest) && (ten_kappa.wrapping_sub(rest.wrapping_mul(2)) >= unit.wrapping_mul(2)) {
        return true;
    }
    // If 2 * (rest - unit) >= 10^kappa, then we can safely round up.
    if (rest > unit) && (ten_kappa - (rest - unit) <= (rest - unit)) {
        // Increment the last digit recursively until we find a non '9' digit.
        buffer.increment(length - 1);
        let mut i = length - 1;
        while i > 0 {
            if buffer.get(i) != b'0' + 10 {
                break;
            }
            buffer.set(i, b'0');
            buffer.increment(i - 1);
            i -= 1;
        }
        // If the first digit is now '0'+ 10 we had a buffer with all '9's. With the exception of
        // the first digit all digits are now '0'. Simply switch the first digit to '1' and adjust
        // the kappa. Example: "99" becomes "10" and the power (the kappa) is increased.
        if buffer.get(0) == b'0' + 10 {
            buffer.set(0, b'1');
            *kappa += 1;
        }
        return true;
    }
    false
}

const TEN4: u32 = 10000;
const TEN5: u32 = 100000;
const TEN6: u32 = 1000000;
const TEN7: u32 = 10000000;
const TEN8: u32 = 100000000;
const TEN9: u32 = 1000000000;

struct DivMagic {
    mul: u32,
    shift: u32,
}

/// This table was computed by libdivide. Essentially, the shift is floor(log2(x)), and the mul is
/// 2^(33 + shift) / x, rounded up and truncated to 32 bits.
///
/// V8's table stops at 10^8 (index 8); the digit loop would read index 9 for an integral part of at
/// least 10^9, which cannot happen: the integral part of `w` after scaling is at most 798,336,123
/// (`dtoa::tests::ten_to_the_nine_is_not_reachable`). A lookup past the table takes the unused
/// entry 0 instead of reading out of bounds.
const DIV: [DivMagic; 9] = [
    DivMagic { mul: 0, shift: 0 },           // Not used, since 1 is not supported by the algorithm.
    DivMagic { mul: 0x9999999a, shift: 3 },  // 10
    DivMagic { mul: 0x47ae147b, shift: 6 },  // 100
    DivMagic { mul: 0x0624dd30, shift: 9 },  // 1000
    DivMagic { mul: 0xa36e2eb2, shift: 13 }, // 10000
    DivMagic { mul: 0x4f8b588f, shift: 16 }, // 100000
    DivMagic { mul: 0x0c6f7a0c, shift: 19 }, // 1000000
    DivMagic { mul: 0xad7f29ac, shift: 23 }, // 10000000
    DivMagic { mul: 0x5798ee24, shift: 26 }, // 100000000
];

/// Returns *val / divisor, and does *val %= divisor. d must be the DivMagic corresponding to the
/// divisor.
///
/// This algorithm is exactly the same as libdivide's branch-free u32 algorithm, except that we add
/// back a branch anyway to support 1.
fn fast_divmod(val: &mut u32, divisor: u32, d: &DivMagic) -> u32 {
    if divisor == 1 {
        let digit = *val;
        *val = 0;
        digit
    } else {
        let q = ((u64::from(*val) * u64::from(d.mul)) >> 32) as u32;
        let t = (val.wrapping_sub(q) >> 1).wrapping_add(q);
        let digit = t >> d.shift;
        *val = val.wrapping_sub(digit.wrapping_mul(divisor));
        digit
    }
}

/// Returns the biggest power of ten that is less than or equal than the given number. We
/// furthermore receive the maximum number of bits 'number' has. If number_bits == 0 then 0^-1 is
/// returned. The number of bits must be <= 32. Precondition: number < (1 << (number_bits + 1)).
/// Returns `(power, exponent)`.
fn biggest_power_ten(number: u32, number_bits: i32) -> (u32, u32) {
    // The C++ `switch` with fallthrough: the cases test the powers from the one the bit count
    // allows downwards; `start` is the first of them.
    let start = match number_bits {
        30..=32 => 9,
        27..=29 => 8,
        24..=26 => 7,
        20..=23 => 6,
        17..=19 => 5,
        14..=16 => 4,
        10..=13 => 3,
        7..=9 => 2,
        4..=6 => 1,
        1..=3 => 0,
        0 => return (0, u32::MAX),
        // Following assignments are here to silence compiler warnings (V8: UNREACHABLE).
        _ => return (0, 0),
    };
    let powers = [(1, 0), (10, 1), (100, 2), (1000, 3), (TEN4, 4), (TEN5, 5), (TEN6, 6), (TEN7, 7), (TEN8, 8), (TEN9, 9)];
    let mut i = start;
    loop {
        let (power, exponent) = powers[i];
        if power <= number {
            return (power, exponent);
        }
        if i == 0 {
            // case 0: *power = 0; *exponent = -1;
            return (0, u32::MAX);
        }
        i -= 1;
    }
}

/// Generates (at most) requested_digits of input number w. w is a floating-point number (DiyFp),
/// consisting of a significand and an exponent. Its exponent is bounded by kMinimalTargetExponent
/// and kMaximalTargetExponent. Hence -60 <= w.e() <= -32.
///
/// Returns false if it fails, in which case the generated digits in the buffer should not be used.
/// Preconditions:
///  * w is correct up to 1 ulp (unit in the last place). That is, its error must be strictly less
///    than a unit of its last digit.
///  * kMinimalTargetExponent <= w.e() <= kMaximalTargetExponent
///
/// Postconditions: returns false if procedure fails. otherwise:
///   * buffer is not null-terminated, but length contains the number of digits.
///   * the representation in buffer is the most precise representation of requested_digits digits.
///   * kappa is such that w = buffer * 10^kappa + eps with |eps| < 10^kappa / 2.
///
/// Remark: This procedure takes into account the imprecision of its input numbers. If the precision
/// is not enough to guarantee all the postconditions then false is returned. This usually happens
/// rarely, but the failure-rate increases with higher requested_digits.
fn digit_gen_counted(w: DiyFp, mut requested_digits: i32, buffer: &mut Buffer, length: &mut i32, kappa: &mut i32) -> bool {
    // w is assumed to have an error less than 1 unit. Whenever w is scaled we also scale its error.
    let mut w_error: u64 = 1;
    // We cut the input number into two parts: the integral digits and the fractional digits. We
    // don't emit any decimal separator, but adapt kappa instead. Example: instead of writing "1.2"
    // we put "12" into the buffer and increase kappa by 1.
    let one = DiyFp::new(1u64 << -w.e(), w.e());
    // Division by one is a shift.
    let mut integrals = (w.f() >> -one.e()) as u32;
    // Modulo by one is an and.
    let mut fractionals = w.f() & (one.f() - 1);
    let (mut divisor, divisor_exponent) = biggest_power_ten(integrals, DiyFp::SIGNIFICAND_SIZE - (-one.e()));
    let mut divisor_exponent = divisor_exponent as i32;
    *kappa = divisor_exponent + 1;
    *length = 0;

    // Loop invariant: buffer = w / 10^kappa  (integer division). The invariant holds for the first
    // iteration: kappa has been initialized with the divisor exponent + 1. And the divisor is the
    // biggest power of ten that is smaller than 'integrals'.
    while *kappa > 0 {
        let digit = fast_divmod(&mut integrals, divisor, DIV.get(divisor_exponent as usize).unwrap_or(&DIV[0]));
        buffer.set(*length, b'0' + digit as u8);
        *length += 1;
        requested_digits -= 1;
        *kappa -= 1;
        // Note that kappa now equals the exponent of the divisor and that the invariant thus holds
        // again.
        if requested_digits == 0 {
            break;
        }
        divisor /= 10;
        divisor_exponent -= 1;
    }

    if requested_digits == 0 {
        let rest = (u64::from(integrals) << -one.e()).wrapping_add(fractionals);
        return round_weed_counted(buffer, *length, rest, u64::from(divisor) << -one.e(), w_error, kappa);
    }

    // The integrals have been generated. We are at the point of the decimal separator. In the
    // following loop we simply multiply the remaining digits by 10 and divide by one. We just need
    // to pay attention to multiply associated data (the 'unit'), too. Note that the multiplication
    // by 10 does not overflow, because w.e >= -60 and thus one.e >= -60.
    while requested_digits > 0 && fractionals > w_error {
        fractionals = fractionals.wrapping_mul(10);
        w_error = w_error.wrapping_mul(10);
        // Integer division by one.
        let digit = (fractionals >> -one.e()) as i32;
        buffer.set(*length, b'0' + digit as u8);
        *length += 1;
        requested_digits -= 1;
        fractionals &= one.f() - 1; // Modulo by one.
        *kappa -= 1;
    }
    if requested_digits != 0 {
        return false;
    }
    round_weed_counted(buffer, *length, fractionals, one.f(), w_error, kappa)
}

/// The "counted" version of grisu3 only generates requested_digits number of digits. This version
/// does not generate the shortest representation, and with enough requested digits 0.1 will at some
/// point print as 0.9999999... Grisu3 is too imprecise for real halfway cases (1.5 will not work)
/// and therefore the rounding strategy for halfway cases is irrelevant.
fn grisu3_counted(v: f64, requested_digits: i32, buffer: &mut Buffer, length: &mut i32, decimal_exponent: &mut i32) -> bool {
    let w = Double::new(v).as_normalized_diy_fp();
    let ten_mk_minimal_binary_exponent = MINIMAL_TARGET_EXPONENT - (w.e() + DiyFp::SIGNIFICAND_SIZE);
    let ten_mk_maximal_binary_exponent = MAXIMAL_TARGET_EXPONENT - (w.e() + DiyFp::SIGNIFICAND_SIZE);
    // Cached power of ten: 10^-k, and mk = -k
    let (ten_mk, mk) = get_cached_power_for_binary_exponent_range(ten_mk_minimal_binary_exponent, ten_mk_maximal_binary_exponent);
    // Note that ten_mk is only an approximation of 10^-k. A DiyFp only contains a 64 bit
    // significand and ten_mk is thus only precise up to 64 bits.

    // The DiyFp::Times procedure rounds its result, and ten_mk is approximated too. The variable
    // scaled_w is now off by a small amount. In fact: scaled_w - w*10^k < 1ulp (unit in the last
    // place) of scaled_w. In other words: let f = scaled_w.f() and e = scaled_w.e(), then
    //           (f-1) * 2^e < w*10^k < (f+1) * 2^e
    let scaled_w = DiyFp::times(w, ten_mk);

    // We now have (double) (scaled_w * 10^-mk). DigitGen will generate the first requested_digits
    // digits of scaled_w and return together with a kappa such that scaled_w ~= buffer * 10^kappa.
    // (It will not always be exactly the same since DigitGenCounted only produces a limited number
    // of digits.)
    let mut kappa = 0;
    let result = digit_gen_counted(scaled_w, requested_digits, buffer, length, &mut kappa);
    *decimal_exponent = -mk + kappa;
    result
}

/// `FastDtoa(v, FAST_DTOA_PRECISION, requested_digits, buffer, &length, &decimal_point)`: provides
/// a decimal representation of the positive finite `v`. Returns true if it succeeds, otherwise the
/// result cannot be trusted. If it returns true then `v == (double) (buffer * 10^(point - length))`.
pub(super) fn fast_dtoa_precision(v: f64, requested_digits: i32, buffer: &mut Buffer, length: &mut i32, decimal_point: &mut i32) -> bool {
    let mut decimal_exponent = 0;
    let mut local_length = 0;
    let result = grisu3_counted(v, requested_digits, buffer, &mut local_length, &mut decimal_exponent);
    *length = local_length;
    if result {
        *decimal_point = *length + decimal_exponent;
        buffer.set(*length, 0);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn div_table_follows_the_libdivide_rule() {
        // libdivide's u32 magic as the table comment states it: shift = floor(log2(x)),
        // mul = ceil(2^(33 + shift) / x) truncated to 32 bits.
        for (i, entry) in DIV.iter().enumerate().skip(1) {
            let x = 10u128.pow(i as u32);
            let shift = 127 - x.leading_zeros();
            let mul = ((1u128 << (33 + shift)).div_ceil(x)) as u32;
            assert_eq!((entry.mul, entry.shift), (mul, shift), "10^{i}");
        }
    }

    #[test]
    fn fast_divmod_divides() {
        for (i, entry) in DIV.iter().enumerate().skip(1) {
            let divisor = 10u32.pow(i as u32);
            let mut state = 0x1234_5678_9ABC_DEF1u64;
            for _ in 0..20_000 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let v = (state >> 16) as u32;
                let mut r = v;
                let q = fast_divmod(&mut r, divisor, entry);
                assert_eq!((q, r), (v / divisor, v % divisor), "{v} / {divisor}");
            }
        }
    }

    #[test]
    fn biggest_power_ten_matches_the_definition() {
        for bits in 1..=32 {
            let max = (1u64 << bits) - 1;
            for n in [0u64, 1, 9, 10, 99, 100, 999, 1000, 9999, 10_000, 99_999, 999_999_999, 1_000_000_000, max] {
                if n > max {
                    continue;
                }
                let (power, exponent) = biggest_power_ten(n as u32, bits);
                if n == 0 {
                    assert_eq!((power, exponent), (0, u32::MAX), "bits {bits}");
                } else {
                    assert_eq!(power, 10u32.pow(exponent), "n {n} bits {bits}");
                    assert!(u64::from(power) <= n && n < u64::from(power) * 10, "n {n} bits {bits}: power {power}");
                }
            }
        }
    }
}
