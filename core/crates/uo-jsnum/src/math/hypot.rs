//! `Math.hypot`: V8's `FastMathHypot` (up to 3 arguments) and the Kahan slow path of
//! `MathHypot` (`src/builtins/math.tq:399–510`). Infinity is checked before NaN.

use super::max;

/// `Math.hypot(a, b)`.
#[must_use]
pub fn hypot(a: f64, b: f64) -> f64 {
    let a = a.abs();
    let b = b.abs();
    if a == f64::INFINITY || b == f64::INFINITY {
        return f64::INFINITY;
    }
    let m = max(a, b);
    if m.is_nan() {
        return f64::NAN;
    }
    if m == 0.0 {
        return 0.0;
    }
    ((a / m) * (a / m) + (b / m) * (b / m)).sqrt() * m
}

/// `Math.hypot(a, b, c)`: note the compensation term, this is not the Kahan loop.
#[must_use]
pub fn hypot3(a: f64, b: f64, c: f64) -> f64 {
    let a = a.abs();
    let b = b.abs();
    let c = c.abs();
    if a == f64::INFINITY || b == f64::INFINITY || c == f64::INFINITY {
        return f64::INFINITY;
    }
    let m = max(max(a, b), c);
    if m.is_nan() {
        return f64::NAN;
    }
    if m == 0.0 {
        return 0.0;
    }
    let power_a = (a / m) * (a / m);
    let power_b = (b / m) * (b / m);
    let compensation = (power_a + power_b) - power_a - power_b;
    let power_c = (c / m) * (c / m) - compensation;
    (power_a + power_b + power_c).sqrt() * m
}

/// `Math.hypot(...v)`: 0 → 0, 1 → |a|, 2 and 3 → the fast paths, more → Kahan summation.
#[must_use]
pub fn hypot_n(v: &[f64]) -> f64 {
    match *v {
        [] => 0.0,
        [a] => a.abs(),
        [a, b] => hypot(a, b),
        [a, b, c] => hypot3(a, b, c),
        _ => hypot_slow(v),
    }
}

fn hypot_slow(v: &[f64]) -> f64 {
    let mut one_arg_is_nan = false;
    let mut max = 0.0;
    for &value in v {
        if value.is_nan() {
            one_arg_is_nan = true;
        } else {
            let abs_value = value.abs();
            if abs_value > max {
                max = abs_value;
            }
        }
    }
    if max == f64::INFINITY {
        return f64::INFINITY;
    } else if one_arg_is_nan {
        return f64::NAN;
    } else if max == 0.0 {
        return 0.0;
    }

    let mut sum = 0.0;
    let mut compensation = 0.0;
    for &value in v {
        let n = value.abs() / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    f64::sqrt(sum) * max
}
