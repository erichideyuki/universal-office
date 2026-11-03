//! `Math.pow` and `**` as Node 24.15.0 (V8 13.6.233.17) computes them on linux-x64 with glibc
//! 2.39 and its non-FMA `pow` (gate 1 decision G1 (a), reference pinned by G2).
//!
//! Two parts, translated statement by statement:
//!
//! - [`pow`]: the `use_std_math_pow` branch of `v8::internal::math::pow`
//!   (`src/numbers/ieee754.cc` at tag `13.6.233.17`, BSD-3-Clause, see `LICENSE-V8`): the ES
//!   special cases V8 handles before calling the C library, and the shortcuts that keep the
//!   interpreter equal to the optimizing compilers (`y == 2`, `y == 0.5`).
//! - [`core`] (and the tables in `pow/log_data.rs`, `pow/exp_data.rs`): what glibc 2.39 runs for
//!   `std::pow` once its FMA variant is masked (`GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA,-AVX2`
//!   selects `__ieee754_pow_sse2`). Its source is ARM optimized-routines `math/pow.c`,
//!   `math/pow_log_data.c`, `math/exp_data.c` and `math/math_config.h` at commit
//!   `c0137013d868422fce51de5e5a2af567ff784b7e` (MIT, see `LICENSE-optimized-routines`), whose
//!   algorithm and tables equal glibc's `sysdeps/ieee754/dbl-64/e_pow.c` (the glibc copy was only
//!   compared, never used). Configuration: `HAVE_FAST_FMA` 0 (the non-FMA branches everywhere,
//!   whatever the target supports), `TOINT_INTRINSICS` 0, `EXP_USE_TOINT_NARROW` 0,
//!   `EXP_POLY_ORDER` 5, `POW_LOG_POLY_ORDER` 8, `WANT_ROUNDING` 1. `WANT_ERRNO` only sets
//!   `errno` and does not change any value. NaN payloads are not part of the contract (NaN is
//!   canonicalized before any bit inspection).
//!
//! The order of every floating-point operation is the C order (left to right, no
//! reassociation); Rust never contracts `a * b + c` into an FMA. C unsigned arithmetic is
//! written with `wrapping_*`.

mod exp_data;
mod log_data;

/// `Math.pow(x, y)` and `x ** y`.
pub fn pow(x: f64, y: f64) -> f64 {
    if y.is_nan() {
        // 1. If exponent is NaN, return NaN.
        return f64::NAN;
    }
    if y.is_infinite() && (x == 1.0 || x == -1.0) {
        // 9.b / 10.b: exponent ±∞ and abs(base) = 1 → NaN.
        return f64::NAN;
    }
    // std::pow distinguishes quiet and signaling NaN; JS does not.
    let x = if x.is_nan() { f64::NAN } else { x };

    // Shortcuts that match the optimizing compilers, which avoid calling pow for these.
    if y == 2.0 {
        return x * x;
    } else if y == 0.5 {
        if x.is_infinite() {
            return f64::INFINITY;
        }
        // + 0 so that (-0) ** 0.5 is +0, not -0.
        return (x + 0.0).sqrt();
    }

    core(x, y)
}

/// `asuint64`.
#[inline(always)]
fn asuint64(x: f64) -> u64 {
    x.to_bits()
}

/// `asdouble`.
#[inline(always)]
fn asdouble(i: u64) -> f64 {
    f64::from_bits(i)
}

/// Top 12 bits of a double (sign and exponent bits).
#[inline(always)]
fn top12(x: f64) -> u32 {
    (asuint64(x) >> 52) as u32
}

/// `OFF`: the start of the subinterval split of the log reduction.
const OFF: u64 = 0x3fe6_9555_0000_0000;
/// `0x1p1009`
const P1009: f64 = f64::from_bits(0x7f00_0000_0000_0000);
/// `0x1p-1022`
const PM1022: f64 = f64::from_bits(0x0010_0000_0000_0000);
/// `0x1p52`
const P52: f64 = f64::from_bits(0x4330_0000_0000_0000);
/// `0x1p769` (`__math_oflow`)
const P769: f64 = f64::from_bits(0x7000_0000_0000_0000);
/// `0x1p-767` (`__math_uflow`)
const PM767: f64 = f64::from_bits(0x1000_0000_0000_0000);
/// `SIGN_BIAS` = `0x800 << EXP_TABLE_BITS`.
const SIGN_BIAS: u32 = 0x800 << 7;

/// `xflow` of `math_err.c`: `(sign ? -y : y) * y`.
#[inline(always)]
fn xflow(sign: u32, y: f64) -> f64 {
    (if sign != 0 { -y } else { y }) * y
}

/// `__math_oflow`: ±∞.
#[inline(always)]
fn math_oflow(sign: u32) -> f64 {
    xflow(sign, P769)
}

/// `__math_uflow`: ±0.
#[inline(always)]
fn math_uflow(sign: u32) -> f64 {
    xflow(sign, PM767)
}

/// `__math_invalid(x)` = `(x - x) / (x - x)` for a finite `x`: NaN (payload not observable).
#[inline(always)]
fn math_invalid(_x: f64) -> f64 {
    f64::NAN
}

/// Computes `y + tail = log(x)` where the rounded result is `y` and `tail` has about 15 bits
/// more precision. `ix` is the bit representation of `x`, normalized in the subnormal range
/// using the sign bit for the exponent. Non-FMA branch.
#[inline(always)]
fn log_inline(ix: u64) -> (f64, f64) {
    use log_data::{LN2HI, LN2LO, N, POLY as A, TAB as T};

    // x = 2^k z; where z is in range [OFF,2*OFF) and exact.
    let tmp = ix.wrapping_sub(OFF);
    let i = ((tmp >> (52 - 7)) % N) as usize;
    let k = ((tmp as i64) >> 52) as i32; // arithmetic shift
    let iz = ix.wrapping_sub(tmp & (0xfff_u64 << 52));
    let z = asdouble(iz);
    let kd = f64::from(k);

    // log(x) = k*Ln2 + log(c) + log1p(z/c-1).
    let invc = T[i].invc;
    let logc = T[i].logc;
    let logctail = T[i].logctail;

    // Split z such that rhi, rlo and rhi*rhi are exact and |rlo| <= |r|.
    let zhi = asdouble(iz.wrapping_add(1_u64 << 31) & (u64::MAX << 32));
    let zlo = z - zhi;
    let rhi = zhi * invc - 1.0;
    let rlo = zlo * invc;
    let r = rhi + rlo;

    // k*Ln2 + log(c) + r.
    let t1 = kd * LN2HI + logc;
    let t2 = t1 + r;
    let lo1 = kd * LN2LO + logctail;
    let lo2 = t1 - t2 + r;

    // Evaluation is optimized assuming superscalar pipelined execution.
    let ar = A[0] * r; // A[0] = -0.5.
    let ar2 = r * ar;
    let ar3 = r * ar2;
    // k*Ln2 + log(c) + r + A[0]*r*r.
    let arhi = A[0] * rhi;
    let arhi2 = rhi * arhi;
    let hi = t2 + arhi2;
    let lo3 = rlo * (ar + arhi);
    let lo4 = t2 - hi + arhi2;
    // p = log1p(r) - r - A[0]*r*r.
    let p = ar3 * (A[1] + r * A[2] + ar2 * (A[3] + r * A[4] + ar2 * (A[5] + r * A[6])));
    let lo = lo1 + lo2 + lo3 + lo4 + p;
    let y = hi + lo;
    let tail = hi - y + lo;
    (y, tail)
}

/// Handles cases that may overflow or underflow when computing the result that is
/// `scale * (1 + tmp)` without intermediate rounding. The bit representation of scale is in
/// `sbits`, but its computed exponent may have overflowed into the sign bit, so it is adjusted
/// before use. `(int32_t) ki` is the k of the argument reduction: positive means the result may
/// overflow, negative that it may underflow.
#[inline(never)]
fn specialcase(tmp: f64, mut sbits: u64, ki: u64) -> f64 {
    if (ki & 0x8000_0000) == 0 {
        // k > 0, the exponent of scale might have overflowed by <= 460.
        sbits = sbits.wrapping_sub(1009_u64 << 52);
        let scale = asdouble(sbits);
        // (check_oflow is the identity on the value)
        return P1009 * (scale + scale * tmp);
    }
    // k < 0, need special care in the subnormal range.
    sbits = sbits.wrapping_add(1022_u64 << 52);
    // Note: sbits is signed scale.
    let scale = asdouble(sbits);
    let mut y = scale + scale * tmp;
    if y.abs() < 1.0 {
        // Round y to the right precision before scaling it into the subnormal range to avoid
        // double rounding.
        let one = if y < 0.0 { -1.0 } else { 1.0 };
        let lo = scale - y + scale * tmp;
        let hi = one + y;
        let lo = one - hi + y + lo; // not `lo += …`: that would reorder the sum
        y = (hi + lo) - one;
        // Fix the sign of 0.
        if y == 0.0 {
            y = asdouble(sbits & 0x8000_0000_0000_0000);
        }
        // (the explicit underflow exception has no value effect)
    }
    // (check_uflow is the identity on the value)
    PM1022 * y
}

/// Computes `sign * exp(x + xtail)` where `|xtail| < 2^-8/N` and `|xtail| <= |x|`. `sign_bias`
/// is `SIGN_BIAS` or 0 and sets the sign to -1 or 1.
#[inline(always)]
fn exp_inline(x: f64, xtail: f64, sign_bias: u32) -> f64 {
    use exp_data::{INV_LN2_N, N, NEG_LN2_HI_N, NEG_LN2_LO_N, POLY, SHIFT, TAB as T};
    let c2 = POLY[0];
    let c3 = POLY[1];
    let c4 = POLY[2];
    let c5 = POLY[3];

    let mut abstop = top12(x) & 0x7ff;
    if abstop.wrapping_sub(top12(f64::from_bits(0x3c90_0000_0000_0000)))
        >= top12(512.0).wrapping_sub(top12(f64::from_bits(0x3c90_0000_0000_0000)))
    {
        // (0x3c90000000000000 = 0x1p-54)
        if abstop.wrapping_sub(top12(f64::from_bits(0x3c90_0000_0000_0000))) >= 0x8000_0000 {
            // Avoid spurious underflow for tiny x. Note: 0 is common input.
            let one = 1.0 + x; // WANT_ROUNDING
            return if sign_bias != 0 { -one } else { one };
        }
        if abstop >= top12(1024.0) {
            // Note: inf and nan are already handled.
            if asuint64(x) >> 63 != 0 {
                return math_uflow(sign_bias);
            } else {
                return math_oflow(sign_bias);
            }
        }
        // Large x is special cased below.
        abstop = 0;
    }

    // exp(x) = 2^(k/N) * exp(r), with exp(r) in [2^(-1/2N),2^(1/2N)].
    // x = ln2/N*k + r, with int k and r in [-ln2/2N, ln2/2N].
    let z = INV_LN2_N * x;
    // z - kd is in [-1, 1] in non-nearest rounding modes.
    let mut kd = z + SHIFT;
    let ki = asuint64(kd);
    kd -= SHIFT;
    let mut r = x + kd * NEG_LN2_HI_N + kd * NEG_LN2_LO_N;
    // The code assumes 2^-200 < |xtail| < 2^-8/N.
    r += xtail;
    // 2^(k/N) ~= scale * (1 + tail).
    let idx = (2 * (ki % N)) as usize;
    let top = ki.wrapping_add(u64::from(sign_bias)) << (52 - 7);
    let tail = asdouble(T[idx]);
    // This is only a valid scale when -1023*N < k < 1024*N.
    let sbits = T[idx + 1].wrapping_add(top);
    // exp(x) = 2^(k/N) * exp(r) ~= scale + scale * (tail + exp(r) - 1).
    let r2 = r * r;
    // Without fma the worst case error is 0.25/N ulp larger.
    let tmp = tail + r + r2 * (c2 + r * c3) + r2 * r2 * (c4 + r * c5);
    if abstop == 0 {
        return specialcase(tmp, sbits, ki);
    }
    let scale = asdouble(sbits);
    // Note: tmp == 0 or |tmp| > 2^-200 and scale > 2^-739, so there is no spurious underflow
    // here even without fma.
    scale + scale * tmp
}

/// Returns 0 if not int, 1 if odd int, 2 if even int. The argument is the bit representation
/// of a non-zero finite floating-point value.
#[inline(always)]
fn checkint(iy: u64) -> i32 {
    let e = ((iy >> 52) & 0x7ff) as i32;
    if e < 0x3ff {
        return 0;
    }
    if e > 0x3ff + 52 {
        return 2;
    }
    if iy & ((1_u64 << (0x3ff + 52 - e)) - 1) != 0 {
        return 0;
    }
    if iy & (1_u64 << (0x3ff + 52 - e)) != 0 {
        return 1;
    }
    2
}

/// True if `i` is the bit representation of 0, infinity or nan.
#[inline(always)]
fn zeroinfnan(i: u64) -> bool {
    (i << 1).wrapping_sub(1) >= (asuint64(f64::INFINITY) << 1).wrapping_sub(1)
}

/// The optimized-routines `pow`, non-FMA branch (glibc 2.39 `__ieee754_pow_sse2`).
fn core(x: f64, y: f64) -> f64 {
    let mut sign_bias: u32 = 0;
    let mut ix = asuint64(x);
    let iy = asuint64(y);
    let mut topx = top12(x);
    let topy = top12(y);
    if topx.wrapping_sub(0x001) >= 0x7ff - 0x001 || (topy & 0x7ff).wrapping_sub(0x3be) >= 0x43e - 0x3be {
        // Note: if |y| > 1075 * ln2 * 2^53 ~= 0x1.749p62 then pow(x,y) = inf/0 and if
        // |y| < 2^-54 / 1075 ~= 0x1.e7b6p-65 then pow(x,y) = +-1.
        // Special cases: (x < 0x1p-126 or inf or nan) or (|y| < 0x1p-65 or |y| >= 0x1p63 or nan).
        if zeroinfnan(iy) {
            if (iy << 1) == 0 {
                return 1.0; // (x is never a signaling NaN here: V8 quiets it)
            }
            if ix == asuint64(1.0) {
                return 1.0; // (y is not NaN here: handled by the V8 wrapper)
            }
            if (ix << 1) > (asuint64(f64::INFINITY) << 1) || (iy << 1) > (asuint64(f64::INFINITY) << 1) {
                return x + y;
            }
            if (ix << 1) == (asuint64(1.0) << 1) {
                return 1.0;
            }
            if ((ix << 1) < (asuint64(1.0) << 1)) == ((iy >> 63) == 0) {
                return 0.0; // |x|<1 && y==inf or |x|>1 && y==-inf.
            }
            return y * y;
        }
        if zeroinfnan(ix) {
            let mut x2 = x * x;
            if ix >> 63 != 0 && checkint(iy) == 1 {
                x2 = -x2;
            }
            return if iy >> 63 != 0 { 1.0 / x2 } else { x2 };
        }
        // Here x and y are non-zero finite.
        if ix >> 63 != 0 {
            // Finite x < 0.
            let yint = checkint(iy);
            if yint == 0 {
                return math_invalid(x);
            }
            if yint == 1 {
                sign_bias = SIGN_BIAS;
            }
            ix &= 0x7fff_ffff_ffff_ffff;
            topx &= 0x7ff;
        }
        if (topy & 0x7ff).wrapping_sub(0x3be) >= 0x43e - 0x3be {
            // Note: sign_bias == 0 here because y is not odd.
            if ix == asuint64(1.0) {
                return 1.0;
            }
            if (topy & 0x7ff) < 0x3be {
                // |y| < 2^-65, x^y ~= 1 + y*log(x).
                return if ix > asuint64(1.0) { 1.0 + y } else { 1.0 - y };
            }
            return if (ix > asuint64(1.0)) == (topy < 0x800) { math_oflow(0) } else { math_uflow(0) };
        }
        if topx == 0 {
            // Normalize subnormal x so exponent becomes negative.
            ix = asuint64(x * P52);
            ix &= 0x7fff_ffff_ffff_ffff;
            ix = ix.wrapping_sub(52_u64 << 52);
        }
    }

    let (hi, lo) = log_inline(ix);
    let yhi = asdouble(iy & (u64::MAX << 27));
    let ylo = y - yhi;
    let lhi = asdouble(asuint64(hi) & (u64::MAX << 27));
    let llo = hi - lhi + lo;
    let ehi = yhi * lhi;
    let elo = ylo * lhi + y * llo; // |elo| < |ehi| * 2^-25.
    exp_inline(ehi, elo, sign_bias)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2^e by repeated exact doubling or halving.
    fn two_to(e: i32) -> f64 {
        let mut v = 1.0;
        for _ in 0..e.unsigned_abs() {
            v = if e < 0 { v * 0.5 } else { v * 2.0 };
        }
        v
    }

    #[test]
    fn constants_are_the_source_literals() {
        assert_eq!(P1009, two_to(1009));
        assert_eq!(PM1022, f64::MIN_POSITIVE);
        assert_eq!(P52, 4_503_599_627_370_496.0);
        assert_eq!(P769, two_to(769));
        assert_eq!(PM767, two_to(-767));
        assert_eq!(f64::from_bits(0x3c90_0000_0000_0000), two_to(-54));
        assert_eq!(exp_data::SHIFT, 6_755_399_441_055_744.0);
        assert_eq!(log_data::POLY[0], -0.5);
    }

    #[test]
    fn es_special_cases() {
        assert!(pow(1.0, f64::NAN).is_nan());
        assert!(pow(1.0, f64::INFINITY).is_nan());
        assert!(pow(-1.0, f64::NEG_INFINITY).is_nan());
        assert_eq!(pow(f64::NAN, 0.0), 1.0);
        assert_eq!(pow(f64::NAN, -0.0), 1.0);
        assert!(pow(f64::NAN, 1.0).is_nan());
        assert_eq!(pow(-0.0, 0.5).to_bits(), 0.0_f64.to_bits());
        assert_eq!(pow(f64::NEG_INFINITY, 0.5), f64::INFINITY);
        assert_eq!(pow(-0.0, -3.0), f64::NEG_INFINITY);
        assert_eq!(pow(-0.0, 3.0).to_bits(), (-0.0_f64).to_bits());
        assert_eq!(pow(-0.0, 4.0).to_bits(), 0.0_f64.to_bits());
        assert_eq!(pow(f64::NEG_INFINITY, 3.0), f64::NEG_INFINITY);
        assert_eq!(pow(f64::NEG_INFINITY, -3.0).to_bits(), (-0.0_f64).to_bits());
        assert!(pow(-2.0, 0.5).is_nan());
        assert!(pow(-8.0, 1.0 / 3.0).is_nan());
        assert_eq!(pow(0.5, f64::INFINITY), 0.0);
        assert_eq!(pow(2.0, f64::NEG_INFINITY), 0.0);
        assert_eq!(pow(2.0, f64::INFINITY), f64::INFINITY);
        assert_eq!(pow(-2.0, 3.0), -8.0);
        assert_eq!(pow(10.0, -4.0), 1e-4);
        assert_eq!(pow(2.0, -1074.0), f64::from_bits(1));
        assert_eq!(pow(2.0, 1024.0), f64::INFINITY);
    }
}
