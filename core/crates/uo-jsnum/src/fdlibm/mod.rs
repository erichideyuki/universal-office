//! The fdlibm 5.3 functions of V8 13.6.233.17 (`src/base/ieee754.cc`), translated statement by
//! statement: same operation order, same constants, no fused multiply-add. This is what Node 24
//! runs for every `Math` function except `pow` (record `core/docs/modules/uo-jsnum.md`).
//!
//! Word access and C integer conversions follow the source: `GET_HIGH_WORD(int32_t, d)` is
//! `high_word(d) as i32`, unsigned C arithmetic is `wrapping_*`. `x - x` is how fdlibm makes a NaN
//! from an infinity, and the C index loops stay as they are.
#![allow(clippy::eq_op, clippy::needless_range_loop)]

mod cbrt;
mod explog;
mod hyper;
mod kernels;
mod rem_pio2;
mod trig;

pub use cbrt::cbrt;
pub use explog::{exp, expm1, log, log1p, log2, log10};
pub use hyper::{atanh, cosh, sinh, tanh};
pub use trig::{acos, asin, atan, atan2, cos, sin, tan};

/// Defines `const NAME: f64` from the two hex words of the C comment and a function with the
/// decimal literals of the C source, so a test can check that both spellings are the same
/// double (risk 3 of the plan).
macro_rules! consts {
    ($checks:ident; $($name:ident = $hi:literal, $lo:literal, $dec:literal;)*) => {
        $(const $name: f64 = f64::from_bits(($hi as u64) << 32 | ($lo as u64));)*
        #[cfg(test)]
        #[allow(clippy::excessive_precision, clippy::approx_constant)]
        pub(super) fn $checks() -> Vec<(&'static str, f64, f64)> {
            vec![$((stringify!($name), $name, $dec),)*]
        }
    };
}
pub(crate) use consts;

/// `GET_HIGH_WORD`: the high 32 bits of the double.
#[inline]
pub(crate) fn high_word(d: f64) -> u32 {
    (d.to_bits() >> 32) as u32
}

/// `GET_LOW_WORD`: the low 32 bits of the double.
#[inline]
pub(crate) fn low_word(d: f64) -> u32 {
    d.to_bits() as u32
}

/// `INSERT_WORDS`.
#[inline]
pub(crate) fn from_words(hi: u32, lo: u32) -> f64 {
    f64::from_bits((u64::from(hi) << 32) | u64::from(lo))
}

/// `SET_HIGH_WORD`.
#[inline]
pub(crate) fn set_high_word(d: f64, hi: u32) -> f64 {
    f64::from_bits((d.to_bits() & 0x0000_0000_FFFF_FFFF) | (u64::from(hi) << 32))
}

/// `SET_LOW_WORD`.
#[inline]
pub(crate) fn set_low_word(d: f64, lo: u32) -> f64 {
    f64::from_bits((d.to_bits() & 0xFFFF_FFFF_0000_0000) | u64::from(lo))
}

/// `scalbn` of the C library (fdlibm `s_scalbn.c`): `x * 2^n` rounded once.
pub(crate) fn scalbn(x: f64, n: i32) -> f64 {
    const TWO54: f64 = f64::from_bits(0x4350_0000_0000_0000);
    const TWOM54: f64 = f64::from_bits(0x3C90_0000_0000_0000);
    const HUGE: f64 = 1.0e300;
    const TINY: f64 = 1.0e-300;
    let mut x = x;
    let hx = high_word(x) as i32;
    let lx = low_word(x);
    let mut k = (hx & 0x7ff0_0000) >> 20;
    if k == 0 {
        if (lx | (hx & 0x7fff_ffff) as u32) == 0 {
            return x;
        }
        x *= TWO54;
        let hx2 = high_word(x) as i32;
        k = ((hx2 & 0x7ff0_0000) >> 20) - 54;
        if n < -50_000 {
            return TINY * x;
        }
    }
    if k == 0x7ff {
        return x + x;
    }
    k = k.saturating_add(n);
    if k > 0x7fe {
        return HUGE * f64::copysign(HUGE, x);
    }
    if k > 0 {
        let hx = high_word(x) as i32;
        return set_high_word(x, ((hx & 0x800f_ffffu32 as i32) | (k << 20)) as u32);
    }
    if k <= -54 {
        if n > 50_000 {
            return HUGE * f64::copysign(HUGE, x);
        }
        return TINY * f64::copysign(TINY, x);
    }
    k += 54;
    let hx = high_word(x) as i32;
    let x = set_high_word(x, ((hx & 0x800f_ffffu32 as i32) | (k << 20)) as u32);
    x * TWOM54
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_constants_equal_the_decimal_literals() {
        let mut all = Vec::new();
        all.extend(rem_pio2::checks());
        all.extend(kernels::checks());
        all.extend(trig::checks());
        all.extend(explog::checks());
        all.extend(cbrt::checks());
        assert!(all.len() > 100);
        for (name, hex, dec) in all {
            assert_eq!(hex.to_bits(), dec.to_bits(), "{name}: hex words differ from the decimal literal");
        }
    }

    #[test]
    fn scalbn_matches_ldexp_identities() {
        assert_eq!(scalbn(1.0, 10), 1024.0);
        assert_eq!(scalbn(3.0, -1), 1.5);
        assert_eq!(scalbn(1.0, -1074), f64::from_bits(1));
        assert_eq!(scalbn(1.0, -1075), 0.0);
        assert_eq!(scalbn(f64::from_bits(1), 1074), 1.0);
        assert_eq!(scalbn(1.0, 1024), f64::INFINITY);
        assert_eq!(scalbn(-0.0, 5).to_bits(), (-0.0f64).to_bits());
        assert!(scalbn(f64::NAN, 3).is_nan());
        assert_eq!(scalbn(0.75, -1074), f64::from_bits(1));
    }
}
