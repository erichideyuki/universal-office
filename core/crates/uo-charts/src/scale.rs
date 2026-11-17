//! `scale.ts`: the automatic value-axis scale as the Office does (frontier C-2). Same algorithm and the
//! same floating-point order as the TS; `Math.*` through `uo_jsnum::math`, `toPrecision(12)` through
//! `uo_jsnum::format`.

#![expect(clippy::neg_cmp_op_on_partial_ord, reason = "the TS `!(x > y)` tests: NaN takes the negated branch")]

use uo_jsnum::format::to_precision;
use uo_jsnum::math::{ceil, floor, log, log10, max, min, pow};
use uo_jsnum::parse::to_number;

pub use crate::layout::Scale;

/// `MAX_TICKS`: the most ticks a scale lists and the most divisions a user-fixed unit may make.
pub const MAX_TICKS: f64 = 1000.0;

/// The user-fixed parts of a scale (`fixed` of `autoScale`); `None` is `undefined`. NaN is a value.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Fixed {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub major: Option<f64>,
    pub minor: Option<f64>,
    pub log: Option<f64>,
}

/// JS truthiness of an optional number (`if (fixed.log)`): present, non-zero and not NaN.
fn truthy(x: Option<f64>) -> Option<f64> {
    x.filter(|v| *v != 0.0 && !v.is_nan())
}

/// `+x.toPrecision(12)`: the value without the binary residue (`6 * 0.1`).
fn prec12(x: f64) -> f64 {
    match to_precision(x, 12) {
        Ok(s) => to_number(&s),
        Err(_) => x,
    }
}

/// `niceStep(span, maxDiv)`: the step `{1, 2, 5} * 10^k` that leaves at most `max_div` divisions in `span`.
pub fn nice_step(span: f64, max_div: f64) -> f64 {
    if !(span > 0.0) {
        return 1.0;
    }
    let mut mag = pow(10.0, floor(log10(span / max_div)));
    for _ in 0..8 {
        for f in [1.0, 2.0, 5.0] {
            if span / (f * mag) <= max_div + 1e-9 {
                return f * mag;
            }
        }
        mag *= 10.0;
    }
    span / max_div
}

/// `autoScale(dataMin, dataMax, maxDiv, fixed, pad = 0.05)`: zero enters unless the data are all on one side and
/// far from it (the Office drops zero only when `min > 5/6 max`), `pad` of slack at the end, and the SMALLEST
/// step `{1, 2, 5} * 10^k` with at most `max_div` divisions.
pub fn auto_scale(data_min: f64, data_max: f64, max_div: f64, fixed: &Fixed, pad: f64) -> Scale {
    if let Some(b) = truthy(fixed.log) {
        let pmin = if data_min > 0.0 { data_min } else { 1.0 };
        let pmax = if data_max > 0.0 { data_max } else { b };
        let lo = match fixed.min {
            Some(m) if m > 0.0 => m,
            _ => pow(b, floor(log(pmin) / log(b) + 1e-9)),
        };
        let mut hi = match fixed.max {
            Some(m) if m > lo => m,
            _ => pow(b, ceil(log(pmax) / log(b) - 1e-9)),
        };
        if !(hi > lo) {
            hi = lo * b;
        }
        return Scale { lo, hi, major: 0.0, minor: 0.0, log: Some(b) };
    }
    let mut mn = min(data_min, data_max);
    let mut mx = max(data_min, data_max);
    if !mn.is_finite() || !mx.is_finite() {
        mn = 0.0;
        mx = 1.0;
    }
    if let Some(f) = fixed.min {
        mn = min(mn, f);
    }
    if let Some(f) = fixed.max {
        mx = max(mx, f);
    }
    // zero on the axis, unless the data are all on one side and far from it
    let mut lo0 = mn;
    let mut hi0 = mx;
    // (equal values are not a "narrow range": the axis starts at zero, measured, 5,5,5 -> 0..6)
    if mn >= 0.0 && fixed.min.is_none() {
        lo0 = if mx > mn && (mx - mn) / mx < 1.0 / 6.0 { mn - (mx - mn) / 2.0 } else { 0.0 };
    }
    if mx <= 0.0 && fixed.max.is_none() {
        hi0 = if mn < mx && (mn - mx) / mn < 1.0 / 6.0 { mx + (mx - mn) / 2.0 } else { 0.0 };
    }
    if hi0 == lo0 {
        hi0 = if lo0 == 0.0 { 1.0 } else { lo0 + lo0.abs() * 0.1 };
    }
    let span0 = hi0 - lo0;
    let lo_adj = fixed.min.unwrap_or(if lo0 < 0.0 { lo0 - span0 * pad } else { lo0 });
    let hi_adj = fixed.max.unwrap_or(if hi0 > 0.0 { hi0 + span0 * pad } else { hi0 });
    let mut major = fixed.major.unwrap_or(0.0);
    if !(major > 0.0) || (hi_adj - lo_adj) / major > MAX_TICKS {
        major = nice_step(hi_adj - lo_adj, max_div);
        // rounding the ends to the step can pass maxDiv: raise the step
        for _ in 0..6 {
            let lo = fixed.min.unwrap_or_else(|| floor(lo_adj / major + 1e-9) * major);
            let hi = fixed.max.unwrap_or_else(|| ceil(hi_adj / major - 1e-9) * major);
            if (hi - lo) / major <= max_div + 1e-9 {
                break;
            }
            major = nice_step((hi - lo) * 1.0001, max_div);
        }
    }
    let mut lo = fixed.min.unwrap_or_else(|| floor(lo_adj / major + 1e-9) * major);
    let mut hi = fixed.max.unwrap_or_else(|| ceil(hi_adj / major - 1e-9) * major);
    if fixed.min.is_none() && mn >= 0.0 && lo < 0.0 {
        lo = 0.0;
    }
    if !(hi > lo) {
        hi = lo + major;
    }
    // no binary residue (6 * 0.1 = 0.6000000000000001)
    lo = prec12(lo);
    hi = prec12(hi);
    let minor = fixed.minor.unwrap_or(major / 5.0);
    Scale { lo, hi, major, minor, log: None }
}

/// `tickValues(s, unit = s.major)`: the tick positions (at most [`MAX_TICKS`]).
pub fn tick_values(s: &Scale, unit: Option<f64>) -> Vec<f64> {
    let unit = unit.unwrap_or(s.major);
    let mut out = Vec::new();
    if let Some(log) = truthy(s.log) {
        let step = if unit > 1.0 { unit } else { log };
        let mut v = s.lo;
        while v <= s.hi * (1.0 + 1e-9) && (out.len() as f64) < MAX_TICKS {
            out.push(v);
            v *= step;
        }
        return out;
    }
    if !(unit > 0.0) {
        return out;
    }
    let n = min(MAX_TICKS, floor((s.hi - s.lo) / unit + 1e-9));
    let mut i = 0.0;
    while i <= n {
        let v = prec12(s.lo + i * unit);
        out.push(if v.abs() < unit * 1e-9 { 0.0 } else { v });
        i += 1.0;
    }
    out
}

/// `frac(s, v)`: the fraction 0..1 of the value on the scale (log: values <= 0 go to the base).
pub fn frac(s: &Scale, v: f64) -> f64 {
    if truthy(s.log).is_some() {
        if !(v > 0.0) {
            return 0.0;
        }
        return (log(v) - log(s.lo)) / (log(s.hi) - log(s.lo));
    }
    (v - s.lo) / (s.hi - s.lo)
}
