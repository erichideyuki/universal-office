//! `Double` of `src/base/numbers/double.h`: the parts of a double the digit routines read.

use super::diy_fp::DiyFp;

pub(super) const HIDDEN_BIT: u64 = 0x0010_0000_0000_0000;
const EXPONENT_MASK: u64 = 0x7FF0_0000_0000_0000;
const SIGNIFICAND_MASK: u64 = 0x000F_FFFF_FFFF_FFFF;
/// Excludes the hidden bit.
const PHYSICAL_SIGNIFICAND_SIZE: i32 = 52;
/// Includes the hidden bit.
pub(super) const SIGNIFICAND_SIZE: i32 = 53;
const EXPONENT_BIAS: i32 = 0x3FF + PHYSICAL_SIGNIFICAND_SIZE;
const DENORMAL_EXPONENT: i32 = -EXPONENT_BIAS + 1;

#[derive(Clone, Copy)]
pub(super) struct Double {
    d64: u64,
}

impl Double {
    pub(super) fn new(d: f64) -> Double {
        Double { d64: d.to_bits() }
    }

    /// The value encoded must be strictly greater than 0.
    pub(super) fn as_normalized_diy_fp(self) -> DiyFp {
        let mut f = self.significand();
        let mut e = self.exponent();

        // The current double could be a denormal.
        while (f & HIDDEN_BIT) == 0 {
            f <<= 1;
            e -= 1;
        }
        // Do the final shifts in one go.
        f <<= DiyFp::SIGNIFICAND_SIZE - SIGNIFICAND_SIZE;
        e -= DiyFp::SIGNIFICAND_SIZE - SIGNIFICAND_SIZE;
        DiyFp::new(f, e)
    }

    pub(super) fn as_uint64(self) -> u64 {
        self.d64
    }

    pub(super) fn exponent(self) -> i32 {
        if self.is_denormal() {
            return DENORMAL_EXPONENT;
        }

        let d64 = self.as_uint64();
        let biased_e = ((d64 & EXPONENT_MASK) >> PHYSICAL_SIGNIFICAND_SIZE) as i32;
        biased_e - EXPONENT_BIAS
    }

    pub(super) fn significand(self) -> u64 {
        let d64 = self.as_uint64();
        let significand = d64 & SIGNIFICAND_MASK;
        if !self.is_denormal() { significand + HIDDEN_BIT } else { significand }
    }

    pub(super) fn is_denormal(self) -> bool {
        (self.as_uint64() & EXPONENT_MASK) == 0
    }

    /// Only infinity and NaN are special.
    pub(super) fn is_special(self) -> bool {
        (self.as_uint64() & EXPONENT_MASK) == EXPONENT_MASK
    }
}
