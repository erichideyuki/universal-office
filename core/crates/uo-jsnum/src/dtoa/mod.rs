//! V8's double → decimal digits routines (`src/base/numbers/` at V8 13.6.233.17), the modes
//! `toFixed`, `toPrecision` and `toExponential` use: fixed (digits after the point) and precision
//! (significant digits). The shortest mode (`Number::toString`) is `ryu-js`, see `crate::format`.
//!
//! `DoubleToAscii` (`dtoa.cc`) tries the fast algorithm of each mode (`fixed_dtoa.rs` for fixed,
//! Grisu3 counted in `fast_dtoa.rs` for precision) and falls back to the exact bignum algorithm
//! (`bignum_dtoa.rs`, on `bignum.rs`) when the fast one cannot prove its result. The port keeps V8's
//! statements and their order; C `int` is `i32`, `uint64_t` arithmetic that may wrap is `wrapping_*`.

mod bignum;
mod bignum_dtoa;
mod cached_powers;
mod diy_fp;
mod double;
mod fast_dtoa;
mod fixed_dtoa;

use double::Double;

/// The digits buffer of V8 (`Vector<char>` of the callers): the longest request is toFixed of a
/// value below 1e21 with 100 fraction digits (121 digits); the room is 128.
pub(crate) const CAPACITY: usize = 128;

/// The mode of `DoubleToAscii` (`DtoaMode`) that this port implements.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// `DTOA_FIXED`: `requested_digits` digits after the decimal point.
    Fixed,
    /// `DTOA_PRECISION`: `requested_digits` significant digits.
    Precision,
}

/// The `buffer` of the C routines. Reads and writes outside it are ignored (V8's `Vector` would
/// crash there; the callers' limits keep every index inside: see `CAPACITY`).
pub(crate) struct Buffer {
    bytes: [u8; CAPACITY],
}

impl Buffer {
    pub(crate) const fn new() -> Buffer {
        Buffer { bytes: [0; CAPACITY] }
    }

    /// `buffer[i]`.
    pub(crate) fn get(&self, i: i32) -> u8 {
        usize::try_from(i).ok().and_then(|i| self.bytes.get(i)).copied().unwrap_or(0)
    }

    /// `buffer[i] = value`.
    pub(crate) fn set(&mut self, i: i32, value: u8) {
        if let Some(slot) = usize::try_from(i).ok().and_then(|i| self.bytes.get_mut(i)) {
            *slot = value;
        }
    }

    /// `buffer[i]++`.
    pub(crate) fn increment(&mut self, i: i32) {
        let value = self.get(i);
        self.set(i, value.wrapping_add(1));
    }

    /// The first `length` digits.
    pub(crate) fn digits(&self, length: i32) -> &[u8] {
        self.bytes.get(..usize::try_from(length).unwrap_or(0)).unwrap_or(&[])
    }
}

/// The result of [`double_to_ascii`]: `buffer[..length]` are the digits and the value is
/// `0.digits × 10^point`.
pub(crate) struct Digits {
    pub(crate) buffer: Buffer,
    pub(crate) length: i32,
    pub(crate) point: i32,
}

/// `DoubleToAscii` (`dtoa.cc`) for a finite `v`; the sign of `v` is ignored (the callers keep it).
/// `requested_digits` is the fraction digit count (fixed) or the significant digit count
/// (precision, at least 1 in every call: `DoubleToAscii`'s `requested_digits == 0` answer is not
/// reachable and not ported).
pub(crate) fn double_to_ascii(v: f64, mode: Mode, requested_digits: i32) -> Digits {
    let v = v.abs();
    let mut digits = Digits { buffer: Buffer::new(), length: 0, point: 0 };

    if v == 0.0 {
        digits.buffer.set(0, b'0');
        digits.length = 1;
        digits.point = 1;
        return digits;
    }

    debug_assert!(!Double::new(v).is_special());
    let fast_worked = match mode {
        Mode::Fixed => fixed_dtoa::fast_fixed_dtoa(v, requested_digits, &mut digits.buffer, &mut digits.length, &mut digits.point),
        Mode::Precision => fast_dtoa::fast_dtoa_precision(v, requested_digits, &mut digits.buffer, &mut digits.length, &mut digits.point),
    };
    if fast_worked {
        return digits;
    }

    let bignum_mode = match mode {
        Mode::Fixed => bignum_dtoa::Mode::Fixed,
        Mode::Precision => bignum_dtoa::Mode::Precision,
    };
    bignum_dtoa::bignum_dtoa(v, bignum_mode, requested_digits, &mut digits.buffer, &mut digits.length, &mut digits.point);
    digits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn next(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    /// Whenever the fast algorithm of a mode claims a result, it is the exact (bignum) one: this
    /// proves every table entry of the fast paths, including the one for 10^9 that V8's `div`
    /// table lacks, without any other oracle.
    #[test]
    fn a_fast_result_is_the_exact_result() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let (mut claimed, mut declined) = (0u32, 0u32);
        for i in 0..100_000 {
            let bits = next(&mut state);
            let v = f64::from_bits(bits & 0x7FFF_FFFF_FFFF_FFFF);
            if !v.is_finite() || v == 0.0 {
                continue;
            }
            for &digits in &[1, 2, 3, 5, 8, 11, 15, 16, 17, 18, 20, 25] {
                let mut fast = Digits { buffer: Buffer::new(), length: 0, point: 0 };
                let ok = fast_dtoa::fast_dtoa_precision(v, digits, &mut fast.buffer, &mut fast.length, &mut fast.point);
                if !ok {
                    declined += 1;
                    continue;
                }
                claimed += 1;
                let mut exact = Digits { buffer: Buffer::new(), length: 0, point: 0 };
                bignum_dtoa::bignum_dtoa(v, bignum_dtoa::Mode::Precision, digits, &mut exact.buffer, &mut exact.length, &mut exact.point);
                assert_eq!(
                    (fast.buffer.digits(fast.length), fast.point),
                    (exact.buffer.digits(exact.length), exact.point),
                    "{i}: {v:e} with {digits} digits"
                );
            }
            if i % 4 == 0 {
                let limited = v % 1e21;
                if limited == 0.0 {
                    continue;
                }
                for &fraction in &[0, 1, 2, 5, 10, 15, 20] {
                    let mut fast = Digits { buffer: Buffer::new(), length: 0, point: 0 };
                    if !fixed_dtoa::fast_fixed_dtoa(limited, fraction, &mut fast.buffer, &mut fast.length, &mut fast.point) {
                        continue;
                    }
                    let mut exact = Digits { buffer: Buffer::new(), length: 0, point: 0 };
                    bignum_dtoa::bignum_dtoa(
                        limited,
                        bignum_dtoa::Mode::Fixed,
                        fraction,
                        &mut exact.buffer,
                        &mut exact.length,
                        &mut exact.point,
                    );
                    // the fast fixed algorithm trims zeros, the bignum one may leave trailing ones
                    let trim = |d: &Digits| {
                        let digits = d.buffer.digits(d.length);
                        let end = digits.iter().rposition(|&c| c != b'0').map_or(0, |i| i + 1);
                        (digits.get(..end).unwrap_or(&[]).to_vec(), if end == 0 { 0 } else { d.point })
                    };
                    assert_eq!(trim(&fast), trim(&exact), "{limited:e} with {fraction} fraction digits");
                }
            }
        }
        assert!(claimed > 100_000, "claimed {claimed}, declined {declined}");
        assert!(declined > 0, "Grisu declines some inputs (about 0.5%)");
    }

    /// V8's `div` table of the fast digit loop stops at 10^8: the loop would need 10^9 for an
    /// integral part of at least 10^9 after scaling. For a fixed binary exponent the integral part
    /// grows with the significand, so the largest significand of every exponent bounds it.
    #[test]
    fn ten_to_the_nine_is_not_reachable() {
        let mut best = 0u64;
        for biased in 1..2047u64 {
            let v = f64::from_bits((biased << 52) | 0x000F_FFFF_FFFF_FFFF);
            let w = Double::new(v).as_normalized_diy_fp();
            let (ten_mk, _) = cached_powers::get_cached_power_for_binary_exponent_range(-60 - (w.e() + 64), -32 - (w.e() + 64));
            let scaled = diy_fp::DiyFp::times(w, ten_mk);
            best = best.max(scaled.f() >> -scaled.e());
        }
        // subnormals: the smallest exponent with every significand width
        for width in 1..52 {
            let v = f64::from_bits((1u64 << width) | ((1u64 << width) - 1));
            let w = Double::new(v).as_normalized_diy_fp();
            let (ten_mk, _) = cached_powers::get_cached_power_for_binary_exponent_range(-60 - (w.e() + 64), -32 - (w.e() + 64));
            let scaled = diy_fp::DiyFp::times(w, ten_mk);
            best = best.max(scaled.f() >> -scaled.e());
        }
        assert_eq!(best, 798_336_123);
        assert!(best < 1_000_000_000);
    }
}
