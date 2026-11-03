//! `Math.*` and `**` as V8 13.6.233.17 (Node 24.15.0) computes them.
//!
//! The transcendental functions are the fdlibm 5.3 port of V8 (`crate::fdlibm`), `pow` is in
//! [`pow`], `hypot` follows the Torque builtins, and the rest is exactly defined by ECMA-262.
//! Every `Math.X` of the TS engines maps to `math::X`, including the ones that are exact in
//! IEEE (`floor`, `ceil`, `trunc`, `abs`, `sqrt`, `%`), so a port can be done name for name.

mod hypot;
mod pow;

pub use crate::fdlibm::{acos, asin, atan, atan2, atanh, cbrt, cos, cosh, exp, expm1, log, log1p, log2, log10, sin, sinh, tan, tanh};
pub use hypot::{hypot, hypot_n, hypot3};
pub use pow::pow;

use crate::conv::{to_int32, to_uint32};

/// `Math.round`: halves go towards +∞, and −0.5…−0 gives −0 (`Float64Round` of V8).
#[must_use]
pub fn round(x: f64) -> f64 {
    let r = x.ceil();
    if r - 0.5 > x { r - 1.0 } else { r }
}

/// `Math.sign`: NaN → NaN, ±0 → ±0.
#[must_use]
pub fn sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

/// `Math.max(a, b)`: NaN wins and +0 is larger than −0.
#[must_use]
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() { b } else { a }
    } else if a > b {
        a
    } else {
        b
    }
}

/// `Math.min(a, b)`: NaN wins and −0 is smaller than +0.
#[must_use]
pub fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() { a } else { b }
    } else if a < b {
        a
    } else {
        b
    }
}

/// `Math.max(...v)`: −∞ for an empty list. Never fails, unlike the JS spread above ~120k
/// arguments (accepted divergence, record G15).
#[must_use]
pub fn max_n(v: &[f64]) -> f64 {
    v.iter().fold(f64::NEG_INFINITY, |acc, &x| max(acc, x))
}

/// `Math.min(...v)`: +∞ for an empty list. Never fails (see [`max_n`]).
#[must_use]
pub fn min_n(v: &[f64]) -> f64 {
    v.iter().fold(f64::INFINITY, |acc, &x| min(acc, x))
}

/// `Math.imul`: ToInt32 of both operands, the product modulo 2^32, as a number.
#[must_use]
pub fn imul(a: f64, b: f64) -> f64 {
    f64::from(to_int32(a).wrapping_mul(to_int32(b)))
}

/// `Math.clz32`: leading zero bits of ToUint32(x).
#[must_use]
pub fn clz32(x: f64) -> f64 {
    f64::from(to_uint32(x).leading_zeros())
}

/// `Math.fround`: the nearest f32, as a number (IEEE round to nearest even).
#[must_use]
pub fn fround(x: f64) -> f64 {
    f64::from(x as f32)
}

/// `Math.floor`.
#[must_use]
pub fn floor(x: f64) -> f64 {
    x.floor()
}

/// `Math.ceil`.
#[must_use]
pub fn ceil(x: f64) -> f64 {
    x.ceil()
}

/// `Math.trunc`.
#[must_use]
pub fn trunc(x: f64) -> f64 {
    x.trunc()
}

/// `Math.abs`.
#[must_use]
pub fn abs(x: f64) -> f64 {
    x.abs()
}

/// `Math.sqrt` (exact in IEEE).
#[must_use]
pub fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// The JS `%` operator on numbers: the sign of the dividend, exact (C `fmod`).
#[must_use]
pub fn rem(a: f64, b: f64) -> f64 {
    a % b
}
