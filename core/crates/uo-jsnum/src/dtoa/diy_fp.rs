//! `DiyFp` of `src/base/numbers/diy-fp.h`: a floating-point number with a `u64` significand and
//! an `i32` exponent. Multiplication does not normalize its result.

#[derive(Clone, Copy)]
pub(super) struct DiyFp {
    f: u64,
    e: i32,
}

impl DiyFp {
    pub(super) const SIGNIFICAND_SIZE: i32 = 64;

    pub(super) fn new(f: u64, e: i32) -> DiyFp {
        DiyFp { f, e }
    }

    /// `DiyFp::Times`, the `__SIZEOF_INT128__` branch of V8 (x86-64, aarch64): `Multiply` rounds the same.
    pub(super) fn times(a: DiyFp, b: DiyFp) -> DiyFp {
        let product = u128::from(a.f) * u128::from(b.f);
        let hi = (product >> 64) as u64;
        let lo = product as u64;
        DiyFp { f: hi.wrapping_add(lo >> 63), e: a.e + b.e + 64 }
    }

    /// `DiyFp::Multiply` (diy-fp.cc), the portable branch; kept to cross-check `times` in a test.
    #[cfg(test)]
    fn multiply(mut self, other: DiyFp) -> DiyFp {
        const M32: u64 = 0xFFFF_FFFF;
        let a = self.f >> 32;
        let b = self.f & M32;
        let c = other.f >> 32;
        let d = other.f & M32;
        let ac = a * c;
        let bc = b * c;
        let ad = a * d;
        let bd = b * d;
        let mut tmp = (bd >> 32) + (ad & M32) + (bc & M32);
        // By adding 1U << 31 to tmp we round the final result. Halfway cases will be round up.
        tmp += 1u64 << 31;
        let result_f = ac + (ad >> 32) + (bc >> 32) + (tmp >> 32);
        self.e += other.e + 64;
        self.f = result_f;
        self
    }

    pub(super) fn f(self) -> u64 {
        self.f
    }

    pub(super) fn e(self) -> i32 {
        self.e
    }
}

#[cfg(test)]
mod tests {
    use super::DiyFp;

    #[test]
    fn times_rounds_like_multiply() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..200_000 {
            let a = DiyFp::new(next() | (1 << 63), -60);
            let b = DiyFp::new(next() | (1 << 63), 5);
            let (t, m) = (DiyFp::times(a, b), a.multiply(b));
            assert_eq!((t.f(), t.e()), (m.f(), m.e()));
        }
        let max = DiyFp::new(u64::MAX, 0);
        assert_eq!(DiyFp::times(max, max).f(), max.multiply(max).f());
    }
}
