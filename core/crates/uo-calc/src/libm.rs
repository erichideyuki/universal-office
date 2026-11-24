//! Correctly rounded `ln`, `log10`, `log1p`, `exp`, `expm1`, `pow`, `atan`, `asin`, `acos`, `atan2`, `sinh`, `cosh`
//! in double-double arithmetic (~106 bits), and Excel's x87 `EXP`/`SIN`/`COS`/`TAN` (`functions/libm.ts`).
//!
//! The TS keeps the double-double result in two module globals (`H`, `L`); here every operation returns its pair,
//! which is the same arithmetic in the same order (no fused multiply-add anywhere: Dekker's split). The coefficient
//! tables the TS builds at module load (`1/(2j+1)`, `1/n!`) are `const` here, computed by the same `ddDiv` at
//! compile time (IEEE `+ - * /` are exact-to-spec in const evaluation).
#![expect(clippy::neg_cmp_op_on_partial_ord, reason = "the TS `!(x > y)` tests: NaN takes the negated branch")]
#![expect(clippy::approx_constant, reason = "the TS literals, digit for digit (high halves of exact double-doubles)")]

use uo_jsnum::math::{floor, log2, pow, round, sqrt, trunc};

/// A double-double `(hi, lo)`.
type Dd = (f64, f64);

const SPLIT: f64 = 134_217_729.0; // 2^27 + 1

const fn two_sum(a: f64, b: f64) -> Dd {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}

const fn quick_two_sum(a: f64, b: f64) -> Dd {
    let s = a + b;
    (s, b - (s - a))
}

const fn two_prod(a: f64, b: f64) -> Dd {
    let p = a * b;
    let t = SPLIT * a;
    let ah = t - (t - a);
    let al = a - ah;
    let t = SPLIT * b;
    let bh = t - (t - b);
    let bl = b - bh;
    (p, ((ah * bh - p) + ah * bl + al * bh) + al * bl)
}

/// `(ah, al) + (bh, bl)`.
const fn dd_add(ah: f64, al: f64, bh: f64, bl: f64) -> Dd {
    let (sh, sl) = two_sum(ah, bh);
    let (th, tl) = two_sum(al, bl);
    let (h, l) = quick_two_sum(sh, sl + th);
    quick_two_sum(h, l + tl)
}

/// `(ah, al) · (bh, bl)`.
const fn dd_mul(ah: f64, al: f64, bh: f64, bl: f64) -> Dd {
    let (ph, l) = two_prod(ah, bh);
    let pl = l + (ah * bl + al * bh);
    quick_two_sum(ph, pl)
}

/// `(ah, al) / (bh, bl)`.
const fn dd_div(ah: f64, al: f64, bh: f64, bl: f64) -> Dd {
    let q1 = ah / bh;
    let (h, l) = dd_mul(q1, 0.0, bh, bl);
    let (rh, rl) = dd_add(ah, al, -h, -l);
    let q2 = rh / bh;
    let (h, l) = dd_mul(q2, 0.0, bh, bl);
    let (h, _) = dd_add(rh, rl, -h, -l);
    let q3 = h / bh;
    let (h, l) = quick_two_sum(q1, q2);
    dd_add(h, l, q3, 0.0)
}

const LN2_H: f64 = 0.693_147_180_559_945_3;
const LN2_L: f64 = 2.319_046_813_846_299_6e-17;
const INV_LN10_H: f64 = 0.434_294_481_903_251_8;
const INV_LN10_L: f64 = 1.098_319_650_216_765e-17;

/// `1/(2j+1)` in double-double, j = 0..=22 (atanh series).
const ATANH_C: [Dd; 23] = {
    let mut t = [(0.0, 0.0); 23];
    let mut j = 0;
    while j <= 22 {
        t[j] = dd_div(1.0, 0.0, (2 * j + 1) as f64, 0.0);
        j += 1;
    }
    t
};

/// `1/n!` in double-double, n = 1..=12 (index n − 1; expm1 series).
const FACT_C: [Dd; 12] = {
    let mut t = [(0.0, 0.0); 12];
    let (mut fh, mut fl) = (1.0, 0.0);
    let mut n = 1;
    while n <= 12 {
        let (h, l) = dd_div(fh, fl, n as f64, 0.0);
        fh = h;
        fl = l;
        t[n - 1] = (fh, fl);
        n += 1;
    }
    t
};

/// `1/n!` in double-double, n = 0..=27 (sin/cos series).
const INV_FACT: [Dd; 28] = {
    let mut t = [(0.0, 0.0); 28];
    t[0] = (1.0, 0.0);
    let (mut fh, mut fl) = (1.0, 0.0);
    let mut n = 1;
    while n <= 27 {
        let (h, l) = dd_div(fh, fl, n as f64, 0.0);
        fh = h;
        fl = l;
        t[n] = (fh, fl);
        n += 1;
    }
    t
};

/// `ln(x)` in double-double, `x > 0` finite.
fn ln_dd(x: f64) -> Dd {
    // x = m·2^k with m in [√½; √2)
    let mut x = x;
    let mut k: i32 = 0;
    if x < 2.225_073_858_507_201_4e-308 {
        x *= 18_014_398_509_481_984.0; // subnormal: ×2^54
        k = -54;
    }
    let bits = x.to_bits();
    let hi = (bits >> 32) as u32;
    k += ((hi >> 20) & 0x7ff) as i32 - 1023;
    let m_bits = (u64::from((hi & 0x800f_ffff) | 0x3ff0_0000) << 32) | (bits & 0xffff_ffff);
    let mut m = f64::from_bits(m_bits);
    if m > core::f64::consts::SQRT_2 {
        m /= 2.0;
        k += 1;
    }
    // ln m = 2·atanh(s), s = (m−1)/(m+1)
    let f = m - 1.0; // exact (Sterbenz)
    let (h, l) = two_sum(m, 1.0);
    let (sh, sl) = dd_div(f, 0.0, h, l);
    let (s2h, s2l) = dd_mul(sh, sl, sh, sl);
    let (mut ph, mut pl) = ATANH_C[22];
    for j in (0..=21).rev() {
        let (h, l) = dd_mul(ph, pl, s2h, s2l);
        (ph, pl) = dd_add(h, l, ATANH_C[j].0, ATANH_C[j].1);
    }
    let (h, l) = dd_mul(ph, pl, sh, sl);
    let (lmh, lml) = (2.0 * h, 2.0 * l);
    if k == 0 {
        return (lmh, lml);
    }
    let kf = f64::from(k);
    let (kh, l) = two_prod(kf, LN2_H);
    let kl = l + kf * LN2_L;
    dd_add(kh, kl, lmh, lml)
}

/// `crLog(x)`: correctly rounded `ln x`.
#[must_use]
pub fn cr_log(x: f64) -> f64 {
    if !(x > 0.0) || x == f64::INFINITY {
        return if x == 0.0 {
            f64::NEG_INFINITY
        } else if x == f64::INFINITY {
            f64::INFINITY
        } else {
            f64::NAN
        };
    }
    if x == 1.0 {
        return 0.0;
    }
    let (h, l) = ln_dd(x);
    h + l
}

/// `crLog10(x)`: correctly rounded `log10 x`.
#[must_use]
pub fn cr_log10(x: f64) -> f64 {
    if !(x > 0.0) || x == f64::INFINITY {
        return if x == 0.0 {
            f64::NEG_INFINITY
        } else if x == f64::INFINITY {
            f64::INFINITY
        } else {
            f64::NAN
        };
    }
    if x == 1.0 {
        return 0.0;
    }
    let (h, l) = ln_dd(x);
    let (h, l) = dd_mul(h, l, INV_LN10_H, INV_LN10_L);
    h + l
}

/// `crLog1p(x)`: correctly rounded `ln(1 + x)`.
#[must_use]
pub fn cr_log1p(x: f64) -> f64 {
    if !(x > -1.0) {
        return if x == -1.0 { f64::NEG_INFINITY } else { f64::NAN };
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }
    if x == 0.0 {
        return x;
    }
    let (h, l) = two_sum(1.0, x);
    if h == 1.0 {
        // 1+x rounds to 1: ln(1+x) ≈ x − x²/2
        return if l == 0.0 { 0.0 } else { l - (l * l) / 2.0 };
    }
    let (lh, ll) = ln_dd(h);
    let (h2, l2) = dd_add(lh, ll, l / h, 0.0);
    h2 + l2
}

/// `expm1(r)` in double-double for |r| ≲ 0.35.
fn expm1_small(rh: f64, rl: f64) -> Dd {
    // r' = r/512; Taylor series; back by squaring: e^(2a)−1 = u·(u+2)
    let (ah, al) = (rh / 512.0, rl / 512.0);
    let (mut ph, mut pl) = FACT_C[11];
    for n in (0..=10).rev() {
        let (h, l) = dd_mul(ph, pl, ah, al);
        (ph, pl) = dd_add(h, l, FACT_C[n].0, FACT_C[n].1);
    }
    let (mut uh, mut ul) = dd_mul(ph, pl, ah, al);
    for _ in 0..9 {
        let (h, l) = dd_add(uh, ul, 2.0, 0.0);
        (uh, ul) = dd_mul(uh, ul, h, l);
    }
    (uh, ul)
}

/// `v · 2^k1 · 2^k2` with `k1 = k >> 1` (the TS two-step scaling; `k` can pass 1023/−1022).
fn scale2(v: f64, k: f64) -> f64 {
    let ki = crate::cast::trunc_i32(k); // |k| ≤ ~1100 on every path
    let k1 = ki >> 1;
    let k2 = ki - k1;
    v * pow(2.0, f64::from(k1)) * pow(2.0, f64::from(k2))
}

/// `crExp(x)`: correctly rounded `e^x` (down to the subnormals).
#[must_use]
pub fn cr_exp(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x > 709.782_712_893_384 {
        return f64::INFINITY;
    }
    if x < -745.133_219_101_941_2 {
        return 0.0;
    }
    if x == 0.0 {
        return 1.0;
    }
    let k = round(x / LN2_H);
    let (th, l) = two_prod(k, LN2_H);
    let tl = l + k * LN2_L;
    let (h, l) = dd_add(x, 0.0, -th, -tl);
    let (h, l) = expm1_small(h, l);
    let (h, l) = dd_add(h, l, 1.0, 0.0);
    scale2(h + l, k)
}

/// `e^(zh + zl)` rounded to a double (|z| ≲ 745).
fn exp_of_dd(zh: f64, zl: f64) -> f64 {
    if zh > 709.782_712_893_384 {
        return f64::INFINITY;
    }
    if zh < -745.133_219_101_941_2 {
        return 0.0;
    }
    let k = round(zh / LN2_H);
    let (th, l) = two_prod(k, LN2_H);
    let tl = l + k * LN2_L;
    let (h, l) = dd_add(zh, zl, -th, -tl);
    let (h, l) = expm1_small(h, l);
    let (h, l) = dd_add(h, l, 1.0, 0.0);
    scale2(h + l, k)
}

/// `crPow(x, y)`: correctly rounded `x^y` for `x > 0` (the C library `pow` of the old Analysis ToolPak).
#[must_use]
pub fn cr_pow(x: f64, y: f64) -> f64 {
    if y == 0.0 || x == 1.0 {
        return 1.0;
    }
    if !(x > 0.0) || !x.is_finite() || !y.is_finite() {
        return pow(x, y);
    }
    let (h, l) = ln_dd(x);
    let (h, l) = dd_mul(h, l, y, 0.0);
    exp_of_dd(h, l)
}

/// log2(e) rounded to a 64-bit mantissa (the x87 constant), as an exact double-double.
const L2E64_H: f64 = 1.442_695_040_888_963_4;
const L2E64_L: f64 = 2.038_300_084_272_748_3e-17;

/// Rounds the double-double `(h, l)` to a 64-bit mantissa (ties to even).
fn round64(h: f64, l: f64) -> Dd {
    if h == 0.0 {
        return (0.0, 0.0);
    }
    let ah = h.abs();
    let mut e = floor(log2(ah));
    if pow(2.0, e) > ah {
        e -= 1.0; // log2 may round up
    } else if pow(2.0, e + 1.0) <= ah {
        e += 1.0;
    }
    if ah == pow(2.0, e) && l != 0.0 && (l < 0.0) != (h < 0.0) {
        e -= 1.0; // just below 2^e
    }
    let q = pow(2.0, e - 63.0);
    // h/q is an integer (a multiple of 2^10); the rounding is in l/q
    let nl = l / q;
    let mut k = round(nl);
    if (nl - trunc(nl)).abs() == 0.5 {
        k = 2.0 * round(nl / 2.0); // tie: even (h/q is even)
    }
    quick_two_sum(h, k * q)
}

/// `xlExp(x)`: Excel's `EXP` (x87): `t = x·log2(e)` rounded to 64 bits, result `2^t` rounded to a double.
#[must_use]
pub fn xl_exp(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x > 709.782_712_893_384 {
        return f64::INFINITY;
    }
    if x < -745.133_219_101_941_2 {
        return 0.0;
    }
    if x == 0.0 {
        return 1.0;
    }
    let (h, l) = two_prod(x, L2E64_H);
    let (h, l) = quick_two_sum(h, l + x * L2E64_L);
    let (th, tl) = round64(h, l);
    let n = round(th);
    let (h, l) = dd_add(th, tl, -n, 0.0); // f = t − n, |f| ≤ ½
    let (h, l) = dd_mul(h, l, LN2_H, LN2_L); // f·ln 2
    let (h, l) = expm1_small(h, l);
    let (h, l) = dd_add(h, l, 1.0, 0.0);
    scale2(h + l, n)
}

// ---------------------------------------------------------------- Excel's trigonometry

/// (π with 64 bits)/2 as an exact double-double.
const P2_H: f64 = 1.570_796_326_794_896_6;
const P2_L: f64 = 6.125_742_274_543_1e-17;
/// π/2 as a double-double (for ATAN).
const PIO2_H: f64 = 1.570_796_326_794_896_6;
const PIO2_L: f64 = 6.123_233_995_736_766e-17;

/// Argument reduction by the x87 π: (quadrant, reduced hi, reduced lo).
fn reduce(x: f64) -> (u32, f64, f64) {
    let k = round(x / P2_H);
    if k == 0.0 {
        return (0, x, 0.0);
    }
    let (ph, pl) = two_prod(k, P2_H);
    let (h, l) = dd_add(x, 0.0, -ph, -pl);
    let (h, l) = dd_add(h, l, -k * P2_L, 0.0); // k·P2_L is exact (P2_L has 11 bits)
    // ((k % 4) + 4) % 4 on the integral k
    let q = uo_jsnum::math::rem(uo_jsnum::math::rem(k, 4.0) + 4.0, 4.0);
    (
        if q >= 2.0 {
            if q >= 3.0 { 3 } else { 2 }
        } else if q >= 1.0 {
            1
        } else {
            0
        },
        h,
        l,
    )
}

/// sin(y) in double-double, |y| ≲ π/4.
fn sin_dd(yh: f64, yl: f64) -> Dd {
    let (h, l) = dd_mul(yh, yl, yh, yl);
    let (y2h, y2l) = (-h, -l); // −y²
    let (mut ph, mut pl) = INV_FACT[27];
    for n in (1..=25).rev().step_by(2) {
        let (h, l) = dd_mul(ph, pl, y2h, y2l);
        (ph, pl) = dd_add(h, l, INV_FACT[n].0, INV_FACT[n].1);
    }
    dd_mul(ph, pl, yh, yl)
}

/// cos(y) in double-double, |y| ≲ π/4.
fn cos_dd(yh: f64, yl: f64) -> Dd {
    let (h, l) = dd_mul(yh, yl, yh, yl);
    let (y2h, y2l) = (-h, -l);
    let (mut ph, mut pl) = INV_FACT[26];
    for n in (0..=24).rev().step_by(2) {
        let (h, l) = dd_mul(ph, pl, y2h, y2l);
        (ph, pl) = dd_add(h, l, INV_FACT[n].0, INV_FACT[n].1);
    }
    (ph, pl)
}

/// `xlSin(x)`: Excel's `SIN`.
#[must_use]
pub fn xl_sin(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    if x == 0.0 {
        return x;
    }
    let (q, yh, yl) = reduce(x);
    let (h, l) = if q == 0 || q == 2 { sin_dd(yh, yl) } else { cos_dd(yh, yl) };
    let v = h + l;
    if q >= 2 { -v } else { v }
}

/// `xlCos(x)`: Excel's `COS`.
#[must_use]
pub fn xl_cos(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    let (q, yh, yl) = reduce(x);
    let (h, l) = if q == 0 || q == 2 { cos_dd(yh, yl) } else { sin_dd(yh, yl) };
    let v = h + l;
    if q == 1 || q == 2 { -v } else { v }
}

/// `xlTan(x)`: Excel's `TAN`.
#[must_use]
pub fn xl_tan(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    if x == 0.0 {
        return x;
    }
    let (q, yh, yl) = reduce(x);
    let (sh, sl) = sin_dd(yh, yl);
    let (ch, cl) = cos_dd(yh, yl);
    // tan = sin/cos (odd quadrants: −cos/sin)
    let (h, l) = if q == 0 || q == 2 { dd_div(sh, sl, ch, cl) } else { dd_div(-ch, -cl, sh, sl) };
    h + l
}

/// `√(h + l)` in double-double.
fn sqrt_dd(h: f64, l: f64) -> Dd {
    let s = sqrt(h);
    if s == 0.0 {
        return (0.0, 0.0);
    }
    let (ph, pl) = two_prod(s, s);
    let (rh, _) = dd_add(h, l, -ph, -pl); // remainder a − s²
    let c = rh / (2.0 * s);
    quick_two_sum(s, c)
}

/// `atan(x)` in double-double.
fn atan_dd(xh: f64, xl: f64) -> Dd {
    let (mut xh, mut xl) = (xh, xl);
    let mut neg = false;
    let mut inv = false;
    if xh < 0.0 {
        neg = true;
        xh = -xh;
        xl = -xl;
    }
    if xh > 1.0 {
        (xh, xl) = dd_div(1.0, 0.0, xh, xl);
        inv = true;
    }
    // atan(x) = 2·atan(x/(1+√(1+x²))): up to 3 times → |x| ≤ tan(π/32)
    let mut halvings = 0;
    while halvings < 3 && xh > 0.1 {
        let (h, l) = dd_mul(xh, xl, xh, xl);
        let (h, l) = dd_add(h, l, 1.0, 0.0);
        let (h, l) = sqrt_dd(h, l);
        let (h, l) = dd_add(h, l, 1.0, 0.0);
        (xh, xl) = dd_div(xh, xl, h, l);
        halvings += 1;
    }
    let (h, l) = dd_mul(xh, xl, xh, xl);
    let (x2h, x2l) = (-h, -l);
    let (mut ph, mut pl) = ATANH_C[22];
    for j in (0..=21).rev() {
        let (h, l) = dd_mul(ph, pl, x2h, x2l);
        (ph, pl) = dd_add(h, l, ATANH_C[j].0, ATANH_C[j].1);
    }
    let (h, l) = dd_mul(ph, pl, xh, xl);
    let scale = f64::from(1u32 << halvings);
    let (mut rh, mut rl) = (h * scale, l * scale);
    if inv {
        (rh, rl) = dd_add(PIO2_H, PIO2_L, -rh, -rl);
    }
    if neg { (-rh, -rl) } else { (rh, rl) }
}

/// `crAtan(x)`: correctly rounded `atan x`.
#[must_use]
pub fn cr_atan(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 {
        return x;
    }
    if !x.is_finite() {
        return if x > 0.0 { PIO2_H } else { -PIO2_H };
    }
    let (h, l) = atan_dd(x, 0.0);
    h + l
}

/// `asin(x)` in double-double (|x| ≤ 1): `atan(x/√(1−x²))`.
fn asin_dd(x: f64) -> Dd {
    if x.abs() == 1.0 {
        return (x * PIO2_H, x * PIO2_L);
    }
    let (h, l) = two_prod(x, x);
    let (h, l) = dd_add(1.0, 0.0, -h, -l);
    let (h, l) = sqrt_dd(h, l);
    let (h, l) = dd_div(x, 0.0, h, l);
    atan_dd(h, l)
}

/// `crAsin(x)`: correctly rounded `asin x`.
#[must_use]
pub fn cr_asin(x: f64) -> f64 {
    if !(x.abs() <= 1.0) {
        return f64::NAN;
    }
    if x == 0.0 {
        return x;
    }
    let (h, l) = asin_dd(x);
    h + l
}

/// `crAcos(x)`: `π/2 − asin x` in double-double, rounded once.
#[must_use]
pub fn cr_acos(x: f64) -> f64 {
    if !(x.abs() <= 1.0) {
        return f64::NAN;
    }
    let (h, l) = asin_dd(x);
    let (h, l) = dd_add(PIO2_H, PIO2_L, -h, -l);
    h + l
}

/// `crAtan2(y, x)`: correctly rounded `atan2`.
#[must_use]
pub fn cr_atan2(y: f64, x: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 {
        return if y > 0.0 {
            PIO2_H
        } else if y < 0.0 {
            -PIO2_H
        } else {
            0.0
        };
    }
    let (h, l) = dd_div(y, 0.0, x, 0.0);
    let (mut h, mut l) = atan_dd(h, l);
    if x < 0.0 {
        let (pih, pil) = (2.0 * PIO2_H, 2.0 * PIO2_L);
        (h, l) = if y >= 0.0 { dd_add(h, l, pih, pil) } else { dd_add(h, l, -pih, -pil) };
    }
    h + l
}

/// `e^a` in double-double (not rounded).
fn exp_of_dd_full(a: f64) -> Dd {
    let k = round(a / LN2_H);
    let (th, l) = two_prod(k, LN2_H);
    let tl = l + k * LN2_L;
    let (h, l) = dd_add(a, 0.0, -th, -tl);
    let (h, l) = expm1_small(h, l);
    let (h, l) = dd_add(h, l, 1.0, 0.0);
    let p = pow(2.0, k);
    (h * p, l * p)
}

/// `crSinh(x)`: correctly rounded `sinh` (via expm1 in double-double).
#[must_use]
pub fn cr_sinh(x: f64) -> f64 {
    let a = x.abs();
    if a == 0.0 {
        return x;
    }
    if a > 710.5 {
        return if x > 0.0 { f64::INFINITY } else { f64::NEG_INFINITY };
    }
    // sinh a = (u + u/(u+1))/2, u = e^a − 1
    let (uh, ul) = if a <= 0.34 {
        expm1_small(a, 0.0)
    } else {
        let (eh, el) = exp_of_dd_full(a);
        dd_add(eh, el, -1.0, 0.0)
    };
    let (h, l) = dd_add(uh, ul, 1.0, 0.0);
    let (h, l) = dd_div(uh, ul, h, l);
    let (h, l) = dd_add(uh, ul, h, l);
    let v = (h + l) / 2.0;
    if x < 0.0 { -v } else { v }
}

/// `crCosh(x)`: correctly rounded `cosh`.
#[must_use]
pub fn cr_cosh(x: f64) -> f64 {
    let a = x.abs();
    if a > 710.5 {
        return f64::INFINITY;
    }
    let (eh, el) = exp_of_dd_full(a);
    let (h, l) = dd_div(1.0, 0.0, eh, el);
    let (h, l) = dd_add(eh, el, h, l);
    (h + l) / 2.0
}

/// `crExpm1(x)`: correctly rounded `e^x − 1`.
#[must_use]
pub fn cr_expm1(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 {
        return x;
    }
    if x.abs() <= 0.34 {
        let (h, l) = expm1_small(x, 0.0);
        return h + l;
    }
    if x > 709.782_712_893_384 {
        return f64::INFINITY;
    }
    if x < -40.0 {
        return -1.0;
    }
    let k = round(x / LN2_H);
    let (th, l) = two_prod(k, LN2_H);
    let tl = l + k * LN2_L;
    let (h, l) = dd_add(x, 0.0, -th, -tl);
    let (h, l) = expm1_small(h, l);
    let (h, l) = dd_add(h, l, 1.0, 0.0); // e^r
    let p = pow(2.0, k);
    let (h, l) = dd_add(h * p, l * p, -1.0, 0.0);
    h + l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_bits() {
        // measured rules quoted by the TS comments
        assert_eq!(xl_sin(2.5), 0.598_472_144_103_956_5);
        assert_eq!(xl_cos(PIO2_H), 6.125_742_274_543_1e-17);
        assert_eq!(cr_log(1.0), 0.0);
        assert_eq!(cr_log10(1000.0), 3.0);
        assert_eq!(cr_exp(0.0), 1.0);
        assert_eq!(ATANH_C[0], (1.0, 0.0));
        assert_eq!(INV_FACT[1], (1.0, 0.0));
    }
}
