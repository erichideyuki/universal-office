//! JS numbers that are integers: the `.doc` writer computes character positions with floats where the
//! TS does (`Math.ceil((run.fcEnd - p.fc) / cs)` of a piece with a fractional `fc`) and then uses
//! them as integers. A float → integer `as` cast saturates where JS keeps the value, and the
//! workspace bans it outside `uo_jsnum::conv`; the values here are below 2^53, so they are rebuilt
//! exactly from two `ToUint32` halves.

use uo_jsnum::conv::{is_integer, to_uint32};
use uo_jsnum::math::{abs, floor};

const TWO_32: f64 = 4_294_967_296.0;
const TWO_53: f64 = 9_007_199_254_740_992.0;

/// The integer a JS number holds, when it is one and below 2^53 in magnitude.
#[must_use]
pub fn exact_i64(x: f64) -> Option<i64> {
    if !is_integer(x) || abs(x) >= TWO_53 {
        return None;
    }
    let a = abs(x);
    let hi = floor(a / TWO_32);
    let lo = a - hi * TWO_32;
    let v = (i64::from(to_uint32(hi)) << 32) | i64::from(to_uint32(lo));
    Some(if x < 0.0 { -v } else { v })
}

/// [`exact_i64`] where the caller knows the value is an integer far below 2^53 (positions of a
/// `.doc`: below 2^34); a value that is not one gives 0.
#[must_use]
pub fn int_of(x: f64) -> i64 {
    exact_i64(x).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_round_trip() {
        for v in [0i64, 1, -1, 4_294_967_295, 4_294_967_296, 8_589_934_591, -8_589_934_591, 9_007_199_254_740_991] {
            assert_eq!(exact_i64(v as f64), Some(v));
        }
        assert_eq!(exact_i64(1.5), None);
        assert_eq!(exact_i64(f64::NAN), None);
        assert_eq!(exact_i64(1e300), None);
        assert_eq!(int_of(-0.0), 0);
    }
}
