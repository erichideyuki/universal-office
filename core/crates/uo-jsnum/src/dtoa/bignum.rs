//! `Bignum` of `src/base/numbers/bignum.{h,cc}`: a big unsigned integer of 28-bit "bigits" with a
//! bigit exponent, on the stack (128 bigits, 3584 bits: more than the digit generation of any
//! finite double needs, V8 `DCHECK`s `kMaxSignificantBits >= 324 * 4`). Only what `bignum_dtoa`
//! calls is ported: not the decimal/hex string assignment, `AddUInt64`, `MultiplyByPowerOfTen` or
//! `ToHexString`, which belong to the string → double side.
//!
//! V8 crashes when a bignum would outgrow its array (`EnsureCapacity` → `UNREACHABLE`); here the
//! array is indexed directly, and the largest intermediate value (10^342 × a 53-bit significand
//! shifted by at most 1074 bits, then × 10 once per requested digit, at most 121 times) fits in
//! 128 bigits with room to spare, which the exhaustive no-panic test of `format` covers.

const BIGIT_SIZE: i32 = 28;
const BIGIT_MASK: u32 = (1 << BIGIT_SIZE) - 1;
const MAX_SIGNIFICANT_BITS: i32 = 3584;
const BIGIT_CAPACITY: usize = (MAX_SIGNIFICANT_BITS / BIGIT_SIZE) as usize;
const CHUNK_SIZE: i32 = 32;

pub(super) struct Bignum {
    bigits: [u32; BIGIT_CAPACITY],
    used_digits: i32,
    /// The Bignum's value equals value(bigits) * 2^(exponent * BIGIT_SIZE).
    exponent: i32,
}

impl Bignum {
    pub(super) fn new() -> Bignum {
        Bignum { bigits: [0; BIGIT_CAPACITY], used_digits: 0, exponent: 0 }
    }

    fn bigit(&self, i: i32) -> u32 {
        self.bigits[i as usize]
    }

    fn set_bigit(&mut self, i: i32, value: u32) {
        self.bigits[i as usize] = value;
    }

    /// Guaranteed to lie in one Bigit.
    pub(super) fn assign_uint16(&mut self, value: u16) {
        self.zero();
        if value == 0 {
            return;
        }

        self.bigits[0] = u32::from(value);
        self.used_digits = 1;
    }

    pub(super) fn assign_uint64(&mut self, mut value: u64) {
        const UINT64_SIZE: i32 = 64;

        self.zero();
        if value == 0 {
            return;
        }

        let needed_bigits = UINT64_SIZE / BIGIT_SIZE + 1;
        for i in 0..needed_bigits {
            self.set_bigit(i, (value & u64::from(BIGIT_MASK)) as u32);
            value >>= BIGIT_SIZE;
        }
        self.used_digits = needed_bigits;
        self.clamp();
    }

    pub(super) fn assign_bignum(&mut self, other: &Bignum) {
        self.exponent = other.exponent;
        for i in 0..other.used_digits {
            self.set_bigit(i, other.bigit(i));
        }
        // Clear the excess digits (if there were any).
        for i in other.used_digits..self.used_digits {
            self.set_bigit(i, 0);
        }
        self.used_digits = other.used_digits;
    }

    pub(super) fn subtract_bignum(&mut self, other: &Bignum) {
        // We require this to be bigger than other.
        self.align(other);

        let offset = other.exponent - self.exponent;
        let mut borrow: u32 = 0;
        let mut i = 0;
        while i < other.used_digits {
            let difference = self.bigit(i + offset).wrapping_sub(other.bigit(i)).wrapping_sub(borrow);
            self.set_bigit(i + offset, difference & BIGIT_MASK);
            borrow = difference >> (CHUNK_SIZE - 1);
            i += 1;
        }
        while borrow != 0 {
            let difference = self.bigit(i + offset).wrapping_sub(borrow);
            self.set_bigit(i + offset, difference & BIGIT_MASK);
            borrow = difference >> (CHUNK_SIZE - 1);
            i += 1;
        }
        self.clamp();
    }

    pub(super) fn square(&mut self) {
        let product_length = 2 * self.used_digits;

        // Comba multiplication: compute each column separately.
        let mut accumulator: u64 = 0;
        // First shift the digits so we don't overwrite them.
        let copy_offset = self.used_digits;
        for i in 0..self.used_digits {
            let value = self.bigit(i);
            self.set_bigit(copy_offset + i, value);
        }
        // We have two loops to avoid some 'if's in the loop.
        for i in 0..self.used_digits {
            // Process temporary digit i with power i. The sum of the two indices must be equal to i.
            let mut bigit_index1 = i;
            let mut bigit_index2 = 0;
            // Sum all of the sub-products.
            while bigit_index1 >= 0 {
                let chunk1 = self.bigit(copy_offset + bigit_index1);
                let chunk2 = self.bigit(copy_offset + bigit_index2);
                accumulator = accumulator.wrapping_add(u64::from(chunk1) * u64::from(chunk2));
                bigit_index1 -= 1;
                bigit_index2 += 1;
            }
            self.set_bigit(i, (accumulator as u32) & BIGIT_MASK);
            accumulator >>= BIGIT_SIZE;
        }
        for i in self.used_digits..product_length {
            let mut bigit_index1 = self.used_digits - 1;
            let mut bigit_index2 = i - bigit_index1;
            // Invariant: sum of both indices is again equal to i. Inner loop runs 0 times on last
            // iteration, emptying accumulator.
            while bigit_index2 < self.used_digits {
                let chunk1 = self.bigit(copy_offset + bigit_index1);
                let chunk2 = self.bigit(copy_offset + bigit_index2);
                accumulator = accumulator.wrapping_add(u64::from(chunk1) * u64::from(chunk2));
                bigit_index1 -= 1;
                bigit_index2 += 1;
            }
            // The overwritten bigits[i] will never be read in further loop iterations, because
            // bigit_index1 and bigit_index2 are always greater than i - used_digits.
            self.set_bigit(i, (accumulator as u32) & BIGIT_MASK);
            accumulator >>= BIGIT_SIZE;
        }

        // Don't forget to update the used_digits and the exponent.
        self.used_digits = product_length;
        self.exponent *= 2;
        self.clamp();
    }

    pub(super) fn shift_left(&mut self, shift_amount: i32) {
        if self.used_digits == 0 {
            return;
        }
        self.exponent += shift_amount / BIGIT_SIZE;
        let local_shift = shift_amount % BIGIT_SIZE;
        self.bigits_shift_left(local_shift);
    }

    pub(super) fn multiply_by_uint32(&mut self, factor: u32) {
        if factor == 1 {
            return;
        }
        if factor == 0 {
            self.zero();
            return;
        }
        if self.used_digits == 0 {
            return;
        }

        // The product of a bigit with the factor is of size BIGIT_SIZE + 32; with the carry it
        // fits in a double chunk.
        let mut carry: u64 = 0;
        for i in 0..self.used_digits {
            let product = u64::from(factor).wrapping_mul(u64::from(self.bigit(i))).wrapping_add(carry);
            self.set_bigit(i, (product & u64::from(BIGIT_MASK)) as u32);
            carry = product >> BIGIT_SIZE;
        }
        while carry != 0 {
            self.set_bigit(self.used_digits, (carry & u64::from(BIGIT_MASK)) as u32);
            self.used_digits += 1;
            carry >>= BIGIT_SIZE;
        }
    }

    pub(super) fn multiply_by_uint64(&mut self, factor: u64) {
        if factor == 1 {
            return;
        }
        if factor == 0 {
            self.zero();
            return;
        }
        let mut carry: u64 = 0;
        let low = factor & 0xFFFF_FFFF;
        let high = factor >> 32;
        for i in 0..self.used_digits {
            let product_low = low.wrapping_mul(u64::from(self.bigit(i)));
            let product_high = high.wrapping_mul(u64::from(self.bigit(i)));
            let tmp = (carry & u64::from(BIGIT_MASK)).wrapping_add(product_low);
            self.set_bigit(i, (tmp & u64::from(BIGIT_MASK)) as u32);
            carry = (carry >> BIGIT_SIZE).wrapping_add(tmp >> BIGIT_SIZE).wrapping_add(product_high << (32 - BIGIT_SIZE));
        }
        while carry != 0 {
            self.set_bigit(self.used_digits, (carry & u64::from(BIGIT_MASK)) as u32);
            self.used_digits += 1;
            carry >>= BIGIT_SIZE;
        }
    }

    pub(super) fn times10(&mut self) {
        self.multiply_by_uint32(10);
    }

    pub(super) fn assign_power_uint16(&mut self, base: u16, power_exponent: i32) {
        if power_exponent == 0 {
            self.assign_uint16(1);
            return;
        }
        self.zero();
        let mut base = base;
        let mut shifts = 0;
        // We expect base to be in range 2-32, and most often to be 10. It does not make much sense
        // to implement different algorithms for counting the bits.
        while (base & 1) == 0 {
            base >>= 1;
            shifts += 1;
        }
        let mut bit_size = 0;
        let mut tmp_base = i32::from(base);
        while tmp_base != 0 {
            tmp_base >>= 1;
            bit_size += 1;
        }

        // Left to Right exponentiation.
        let mut mask: i32 = 1;
        while power_exponent >= mask {
            mask <<= 1;
        }

        // The mask is now pointing to the bit above the most significant 1-bit of power_exponent.
        // Get rid of first 1-bit;
        mask >>= 2;
        let mut this_value = u64::from(base);

        let mut delayed_multipliciation = false;
        const MAX_32BITS: u64 = 0xFFFF_FFFF;
        while mask != 0 && this_value <= MAX_32BITS {
            this_value = this_value.wrapping_mul(this_value);
            // Verify that there is enough space in this_value to perform the multiplication. The
            // first bit_size bits must be 0.
            if (power_exponent & mask) != 0 {
                let base_bits_mask = !((1u64 << (64 - bit_size)) - 1);
                let high_bits_zero = (this_value & base_bits_mask) == 0;
                if high_bits_zero {
                    this_value = this_value.wrapping_mul(u64::from(base));
                } else {
                    delayed_multipliciation = true;
                }
            }
            mask >>= 1;
        }
        self.assign_uint64(this_value);
        if delayed_multipliciation {
            self.multiply_by_uint32(u32::from(base));
        }

        // Now do the same thing as a bignum.
        while mask != 0 {
            self.square();
            if (power_exponent & mask) != 0 {
                self.multiply_by_uint32(u32::from(base));
            }
            mask >>= 1;
        }

        // And finally add the saved shifts.
        self.shift_left(shifts * power_exponent);
    }

    /// Precondition: this/other < 16bit. Pseudocode: `result = this / other; this = this % other`.
    /// `other` must not be zero.
    pub(super) fn divide_modulo_int_bignum(&mut self, other: &Bignum) -> u16 {
        // Easy case: if we have less digits than the divisor than the result is 0.
        // Note: this handles the case where this == 0, too.
        if self.bigit_length() < other.bigit_length() {
            return 0;
        }

        self.align(other);

        let mut result: u16 = 0;

        // Start by removing multiples of 'other' until both numbers have the same number of digits.
        while self.bigit_length() > other.bigit_length() {
            // This naive approach is extremely inefficient if the this divided other might be big.
            // This function is implemented for doubleToString where the result should be small
            // (less than 10).
            // Remove the multiples of the first digit.
            // Example this = 23 and other equals 9. -> Remove 2 multiples.
            result = result.wrapping_add(self.bigit(self.used_digits - 1) as u16);
            self.subtract_times(other, self.bigit(self.used_digits - 1) as i32);
        }

        // Both bignums are at the same length now. Since other has more than 0 digits we know that
        // the access to bigits[used_digits - 1] is safe.
        let this_bigit = self.bigit(self.used_digits - 1);
        let other_bigit = other.bigit(other.used_digits - 1);

        if other.used_digits == 1 {
            // Shortcut for easy (and common) case.
            let quotient = (this_bigit / other_bigit) as i32;
            self.set_bigit(self.used_digits - 1, this_bigit.wrapping_sub(other_bigit.wrapping_mul(quotient as u32)));
            result = result.wrapping_add(quotient as u16);
            self.clamp();
            return result;
        }

        let division_estimate = (this_bigit / other_bigit.wrapping_add(1)) as i32;
        result = result.wrapping_add(division_estimate as u16);
        self.subtract_times(other, division_estimate);

        if other_bigit.wrapping_mul((division_estimate + 1) as u32) > this_bigit {
            // No need to even try to subtract. Even if other's remaining digits were 0 another
            // subtraction would be too much.
            return result;
        }

        while Bignum::less_equal(other, self) {
            self.subtract_bignum(other);
            result = result.wrapping_add(1);
        }
        result
    }

    pub(super) fn compare(a: &Bignum, b: &Bignum) -> i32 {
        let bigit_length_a = a.bigit_length();
        let bigit_length_b = b.bigit_length();
        if bigit_length_a < bigit_length_b {
            return -1;
        }
        if bigit_length_a > bigit_length_b {
            return 1;
        }
        let mut i = bigit_length_a - 1;
        while i >= a.exponent.min(b.exponent) {
            let bigit_a = a.bigit_at(i);
            let bigit_b = b.bigit_at(i);
            if bigit_a < bigit_b {
                return -1;
            }
            if bigit_a > bigit_b {
                return 1;
            }
            // Otherwise they are equal up to this digit. Try the next digit.
            i -= 1;
        }
        0
    }

    pub(super) fn equal(a: &Bignum, b: &Bignum) -> bool {
        Bignum::compare(a, b) == 0
    }

    pub(super) fn less_equal(a: &Bignum, b: &Bignum) -> bool {
        Bignum::compare(a, b) <= 0
    }

    /// Returns Compare(a + b, c).
    pub(super) fn plus_compare(a: &Bignum, b: &Bignum, c: &Bignum) -> i32 {
        if a.bigit_length() < b.bigit_length() {
            return Bignum::plus_compare(b, a, c);
        }
        if a.bigit_length() + 1 < c.bigit_length() {
            return -1;
        }
        if a.bigit_length() > c.bigit_length() {
            return 1;
        }
        // The exponent encodes 0-bigits. So if there are more 0-digits in 'a' than 'b' has digits,
        // then the bigit-length of 'a'+'b' must be equal to the one of 'a'.
        if a.exponent >= b.bigit_length() && a.bigit_length() < c.bigit_length() {
            return -1;
        }

        let mut borrow: u32 = 0;
        // Starting at min_exponent all digits are == 0. So no need to compare them.
        let min_exponent = a.exponent.min(b.exponent).min(c.exponent);
        let mut i = c.bigit_length() - 1;
        while i >= min_exponent {
            let chunk_a = a.bigit_at(i);
            let chunk_b = b.bigit_at(i);
            let chunk_c = c.bigit_at(i);
            let sum = chunk_a.wrapping_add(chunk_b);
            if sum > chunk_c.wrapping_add(borrow) {
                return 1;
            }
            borrow = chunk_c.wrapping_add(borrow).wrapping_sub(sum);
            if borrow > 1 {
                return -1;
            }
            borrow <<= BIGIT_SIZE;
            i -= 1;
        }
        if borrow == 0 {
            return 0;
        }
        -1
    }

    /// BigitLength includes the "hidden" digits encoded in the exponent.
    fn bigit_length(&self) -> i32 {
        self.used_digits + self.exponent
    }

    fn bigit_at(&self, index: i32) -> u32 {
        if index >= self.bigit_length() {
            return 0;
        }
        if index < self.exponent {
            return 0;
        }
        self.bigit(index - self.exponent)
    }

    fn clamp(&mut self) {
        while self.used_digits > 0 && self.bigit(self.used_digits - 1) == 0 {
            self.used_digits -= 1;
        }
        if self.used_digits == 0 {
            // Zero.
            self.exponent = 0;
        }
    }

    fn zero(&mut self) {
        for i in 0..self.used_digits {
            self.set_bigit(i, 0);
        }
        self.used_digits = 0;
        self.exponent = 0;
    }

    fn align(&mut self, other: &Bignum) {
        if self.exponent > other.exponent {
            // If "X" represents a "hidden" digit (by the exponent) then we are in the following
            // case (a == this, b == other):
            // a:  aaaaaaXXXX   or a:   aaaaaXXX
            // b:     bbbbbbX      b: bbbbbbbbXX
            // We replace some of the hidden digits (X) of a with 0 digits.
            // a:  aaaaaa000X   or a:   aaaaa0XX
            let zero_digits = self.exponent - other.exponent;
            let mut i = self.used_digits - 1;
            while i >= 0 {
                let value = self.bigit(i);
                self.set_bigit(i + zero_digits, value);
                i -= 1;
            }
            for i in 0..zero_digits {
                self.set_bigit(i, 0);
            }
            self.used_digits += zero_digits;
            self.exponent -= zero_digits;
        }
    }

    /// Requires this to have enough capacity; updates used_digits if necessary. `shift_amount`
    /// must be in `[0, BIGIT_SIZE)`.
    fn bigits_shift_left(&mut self, shift_amount: i32) {
        let mut carry: u32 = 0;
        for i in 0..self.used_digits {
            let new_carry = self.bigit(i) >> (BIGIT_SIZE - shift_amount);
            let value = (self.bigit(i) << shift_amount).wrapping_add(carry) & BIGIT_MASK;
            self.set_bigit(i, value);
            carry = new_carry;
        }
        if carry != 0 {
            self.set_bigit(self.used_digits, carry);
            self.used_digits += 1;
        }
    }

    fn subtract_times(&mut self, other: &Bignum, factor: i32) {
        if factor < 3 {
            for _ in 0..factor {
                self.subtract_bignum(other);
            }
            return;
        }
        let mut borrow: u32 = 0;
        let exponent_diff = other.exponent - self.exponent;
        for i in 0..other.used_digits {
            let product = (factor as u64).wrapping_mul(u64::from(other.bigit(i)));
            let remove = u64::from(borrow).wrapping_add(product);
            let difference = self.bigit(i + exponent_diff).wrapping_sub((remove & u64::from(BIGIT_MASK)) as u32);
            self.set_bigit(i + exponent_diff, difference & BIGIT_MASK);
            borrow = ((difference >> (CHUNK_SIZE - 1)) as u64).wrapping_add(remove >> BIGIT_SIZE) as u32;
        }
        let mut i = other.used_digits + exponent_diff;
        while i < self.used_digits {
            if borrow == 0 {
                return;
            }
            let difference = self.bigit(i).wrapping_sub(borrow);
            self.set_bigit(i, difference & BIGIT_MASK);
            borrow = difference >> (CHUNK_SIZE - 1);
            i += 1;
        }
        self.clamp();
    }
}
