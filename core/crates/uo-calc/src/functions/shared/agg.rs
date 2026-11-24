//! The basic aggregation shapes of `functions/statistical.ts` (`aggAverage`, `aggMax`, `aggMin`, `aggSum`,
//! `aggProduct`, `aggCount`, `aggVarS`, `aggVarP`, `aggStdevS`, `aggStdevP`): SUBTOTAL/AGGREGATE (A4b) and the
//! D-functions of `database.ts` (A4e) share them, so they live here. The rest of the `Agg`s (median, mode,
//! percentiles…) stay with the statistics family.

use uo_jsnum::math::sqrt;

use crate::stats::{stat_num, variance, xl_sum};
use crate::value::ErrorCode;

/// `Agg` = `(xs) => Num`.
pub type Agg = fn(&[f64]) -> Result<f64, ErrorCode>;

/// `aggAverage`.
pub fn average(xs: &[f64]) -> Result<f64, ErrorCode> {
    if xs.is_empty() {
        return Err(ErrorCode::Div0);
    }
    stat_num(xl_sum(xs) / xs.len() as f64)
}

/// `aggMax` (`-Infinity` start, strict `>`: NaN never wins; 0 when empty).
pub fn max(xs: &[f64]) -> Result<f64, ErrorCode> {
    let mut m = f64::NEG_INFINITY;
    for &x in xs {
        if x > m {
            m = x;
        }
    }
    Ok(if xs.is_empty() { 0.0 } else { m })
}

/// `aggMin`.
pub fn min(xs: &[f64]) -> Result<f64, ErrorCode> {
    let mut m = f64::INFINITY;
    for &x in xs {
        if x < m {
            m = x;
        }
    }
    Ok(if xs.is_empty() { 0.0 } else { m })
}

/// `aggSum`.
pub fn sum(xs: &[f64]) -> Result<f64, ErrorCode> {
    stat_num(xl_sum(xs))
}

/// `aggProduct`.
pub fn product(xs: &[f64]) -> Result<f64, ErrorCode> {
    if xs.is_empty() {
        return Ok(0.0);
    }
    let mut p = 1.0;
    for &x in xs {
        p *= x;
    }
    stat_num(p)
}

/// `aggCount`.
pub fn count(xs: &[f64]) -> Result<f64, ErrorCode> {
    Ok(xs.len() as f64)
}

/// `aggVarS`.
pub fn var_s(xs: &[f64]) -> Result<f64, ErrorCode> {
    if xs.len() < 2 { Err(ErrorCode::Div0) } else { stat_num(variance(xs, true)) }
}

/// `aggVarP`.
pub fn var_p(xs: &[f64]) -> Result<f64, ErrorCode> {
    if xs.is_empty() { Err(ErrorCode::Div0) } else { stat_num(variance(xs, false)) }
}

/// `aggStdevS`.
pub fn stdev_s(xs: &[f64]) -> Result<f64, ErrorCode> {
    if xs.len() < 2 { Err(ErrorCode::Div0) } else { stat_num(sqrt(variance(xs, true))) }
}

/// `aggStdevP`.
pub fn stdev_p(xs: &[f64]) -> Result<f64, ErrorCode> {
    if xs.is_empty() { Err(ErrorCode::Div0) } else { stat_num(sqrt(variance(xs, false))) }
}
