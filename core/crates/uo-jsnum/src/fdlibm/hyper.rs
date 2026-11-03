//! `atanh` (ieee754.cc:1557), `cosh` (2554), `sinh` (2930) and `tanh` (2986) of V8's fdlibm.

use super::explog::{exp, expm1, log1p};
use super::{high_word, low_word, set_high_word};

const HUGE: f64 = 1.0e300;
const TINY: f64 = 1.0e-300;

/// `Math.atanh` of V8 13.6.233.17 (fdlibm).
pub fn atanh(x: f64) -> f64 {
    let mut x = x;
    let hx = high_word(x) as i32;
    let lx = low_word(x);
    let ix = hx & 0x7FFF_FFFF;
    if ((ix as u32) | ((lx | lx.wrapping_neg()) >> 31)) > 0x3FF0_0000 {
        return f64::NAN;
    }
    if ix == 0x3FF0_0000 {
        return if x > 0.0 { f64::INFINITY } else { f64::NEG_INFINITY };
    }
    if ix < 0x3E30_0000 && (HUGE + x) > 0.0 {
        return x;
    }
    x = set_high_word(x, ix as u32);
    let t = if ix < 0x3FE0_0000 {
        let t2 = x + x;
        0.5 * log1p(t2 + t2 * x / (1.0 - x))
    } else {
        0.5 * log1p((x + x) / (1.0 - x))
    };
    if hx >= 0 { t } else { -t }
}

/// `Math.cosh` of V8 13.6.233.17 (fdlibm).
pub fn cosh(x: f64) -> f64 {
    const KCOSH_OVERFLOW: f64 = 710.4758600739439;
    let ix = (high_word(x) as i32) & 0x7FFF_FFFF;

    if ix < 0x3FD6_2E43 {
        let t = expm1(x.abs());
        let w = 1.0 + t;
        if ix < 0x3C80_0000 {
            return w;
        }
        return 1.0 + (t * t) / (w + w);
    }

    if ix < 0x4036_0000 {
        let t = exp(x.abs());
        return 0.5 * t + 0.5 / t;
    }

    if ix < 0x4086_2E42 {
        return 0.5 * exp(x.abs());
    }

    if x.abs() <= KCOSH_OVERFLOW {
        let w = exp(0.5 * x.abs());
        let t = 0.5 * w;
        return t * w;
    }

    if ix >= 0x7FF0_0000 {
        return x * x;
    }

    HUGE * HUGE
}

/// `Math.sinh` of V8 13.6.233.17 (fdlibm).
pub fn sinh(x: f64) -> f64 {
    const KSINH_OVERFLOW: f64 = 710.4758600739439;
    const TWO_M28: f64 = 3.725290298461914e-9;
    const LOG_MAXD: f64 = 709.7822265625;
    const SHUGE: f64 = 1.0e307;

    let h = if x < 0.0 { -0.5 } else { 0.5 };
    let ax = x.abs();
    if ax < 22.0 {
        if ax < TWO_M28 {
            return x;
        }
        let t = expm1(ax);
        if ax < 1.0 {
            return h * (2.0 * t - t * t / (t + 1.0));
        }
        return h * (t + t / (t + 1.0));
    }
    if ax < LOG_MAXD {
        return h * exp(ax);
    }
    if ax <= KSINH_OVERFLOW {
        let w = exp(0.5 * ax);
        let t = h * w;
        return t * w;
    }
    x * SHUGE
}

/// `Math.tanh` of V8 13.6.233.17 (fdlibm).
pub fn tanh(x: f64) -> f64 {
    let jx = high_word(x) as i32;
    let ix = jx & 0x7FFF_FFFF;

    if ix >= 0x7FF0_0000 {
        if jx >= 0 {
            return 1.0 / x + 1.0;
        }
        return 1.0 / x - 1.0;
    }

    let z;
    if ix < 0x4036_0000 {
        if ix < 0x3E30_0000 && HUGE + x > 1.0 {
            return x;
        }
        if ix >= 0x3FF0_0000 {
            let t = expm1(2.0 * x.abs());
            z = 1.0 - 2.0 / (t + 2.0);
        } else {
            let t = expm1(-2.0 * x.abs());
            z = -t / (t + 2.0);
        }
    } else {
        z = 1.0 - TINY;
    }
    if jx >= 0 { z } else { -z }
}
