//! `exp` (ieee754.cc:1447), `expm1` (2213), `log` (1638), `log1p` (1783), `log2` (1951, with
//! `k_log1p` 1941) and `log10` (1973) of V8's fdlibm.

use super::{consts, from_words, high_word, low_word, set_high_word, set_low_word};

consts! { checks;
    O_THRESHOLD = 0x40862E42, 0xFEFA39EF, 7.09782712893383973096e+02;
    U_THRESHOLD = 0xC0874910, 0xD52D3051, -7.45133219101941108420e+02;
    LN2HI0 = 0x3FE62E42, 0xFEE00000, 6.93147180369123816490e-01;
    LN2HI1 = 0xBFE62E42, 0xFEE00000, -6.93147180369123816490e-01;
    LN2LO0 = 0x3DEA39EF, 0x35793C76, 1.90821492927058770002e-10;
    LN2LO1 = 0xBDEA39EF, 0x35793C76, -1.90821492927058770002e-10;
    INVLN2 = 0x3FF71547, 0x652B82FE, 1.44269504088896338700e+00;
    P1 = 0x3FC55555, 0x5555553E, 1.66666666666666019037e-01;
    P2 = 0xBF66C16C, 0x16BEBD93, -2.77777777770155933842e-03;
    P3 = 0x3F11566A, 0xAF25DE2C, 6.61375632143793436117e-05;
    P4 = 0xBEBBBD41, 0xC5D26BF1, -1.65339022054652515390e-06;
    P5 = 0x3E663769, 0x72BEA4D0, 4.13813679705723846039e-08;
    E = 0x4005BF0A, 0x8B145769, 2.718281828459045;
    TWOM1000 = 0x01700000, 0x00000000, 9.33263618503218878990e-302;
    TWO1023 = 0x7FE00000, 0x00000000, 8.988465674311579539e307;
    TWO54 = 0x43500000, 0x00000000, 1.80143985094819840000e+16;
    LG1 = 0x3FE55555, 0x55555593, 6.666666666666735130e-01;
    LG2 = 0x3FD99999, 0x9997FA04, 3.999999999940941908e-01;
    LG3 = 0x3FD24924, 0x94229359, 2.857142874366239149e-01;
    LG4 = 0x3FCC71C5, 0x1D8E78AF, 2.222219843214978396e-01;
    LG5 = 0x3FC74664, 0x96CB03DE, 1.818357216161805012e-01;
    LG6 = 0x3FC39A09, 0xD078C69F, 1.531383769920937332e-01;
    LG7 = 0x3FC2F112, 0xDF3E5244, 1.479819860511658591e-01;
    IVLN2HI = 0x3FF71547, 0x65200000, 1.44269504072144627571e+00;
    IVLN2LO = 0x3DE705FC, 0x2EEFA200, 1.67517131648865118353e-10;
    IVLN10 = 0x3FDBCB7B, 0x1526E50E, 4.34294481903251816668e-01;
    LOG10_2HI = 0x3FD34413, 0x509F6000, 3.01029995663611771306e-01;
    LOG10_2LO = 0x3D59FEF3, 0x11F12B36, 3.69423907715893078616e-13;
    Q1 = 0xBFA11111, 0x111110F4, -3.33333333333331316428e-02;
    Q2 = 0x3F5A01A0, 0x19FE5585, 1.58730158725481460165e-03;
    Q3 = 0xBF14CE19, 0x9EAADBB7, -7.93650757867487942473e-05;
    Q4 = 0x3ED0CFCA, 0x86E65239, 4.00821782732936239552e-06;
    Q5 = 0xBE8AFDB7, 0x6E09C32D, -2.01099218183624371326e-07;
}

const HUGE: f64 = 1.0e300;
const TINY: f64 = 1.0e-300;
const HALF_SIGN: [f64; 2] = [0.5, -0.5];
const LN2HI: [f64; 2] = [LN2HI0, LN2HI1];
const LN2LO: [f64; 2] = [LN2LO0, LN2LO1];

/// `Math.exp` of V8 13.6.233.17 (fdlibm).
pub fn exp(x: f64) -> f64 {
    let mut x = x;
    let mut hi = 0.0;
    let mut lo = 0.0;
    let mut k: i32 = 0;

    let mut hx = high_word(x);
    let xsb = ((hx >> 31) & 1) as usize;
    hx &= 0x7FFF_FFFF;

    if hx >= 0x4086_2E42 {
        if hx >= 0x7FF0_0000 {
            let lx = low_word(x);
            if ((hx & 0xF_FFFF) | lx) != 0 {
                return x + x;
            }
            return if xsb == 0 { x } else { 0.0 };
        }
        if x > O_THRESHOLD {
            return HUGE * HUGE;
        }
        if x < U_THRESHOLD {
            return TWOM1000 * TWOM1000;
        }
    }

    if hx > 0x3FD6_2E42 {
        if hx < 0x3FF0_A2B2 {
            if x == 1.0 {
                return E;
            }
            hi = x - LN2HI[xsb];
            lo = LN2LO[xsb];
            k = 1 - (xsb as i32) - (xsb as i32);
        } else {
            k = (INVLN2 * x + HALF_SIGN[xsb]) as i32;
            let t = f64::from(k);
            hi = x - t * LN2HI[0];
            lo = t * LN2LO[0];
        }
        x = hi - lo;
    } else if hx < 0x3E30_0000 {
        if HUGE + x > 1.0 {
            return 1.0 + x;
        }
    } else {
        k = 0;
    }

    let t = x * x;
    let twopk = if k >= -1021 {
        from_words(0x3FF0_0000u32.wrapping_add(((k as u32) << 20) as i32 as u32), 0)
    } else {
        from_words(0x3FF0_0000u32.wrapping_add(((k + 1000) as u32) << 20), 0)
    };
    let c = x - t * (P1 + t * (P2 + t * (P3 + t * (P4 + t * P5))));
    if k == 0 {
        return 1.0 - ((x * c) / (c - 2.0) - x);
    }
    let y = 1.0 - ((lo - (x * c) / (2.0 - c)) - hi);
    if k >= -1021 {
        if k == 1024 {
            return y * 2.0 * TWO1023;
        }
        y * twopk
    } else {
        y * twopk * TWOM1000
    }
}

#[allow(clippy::excessive_precision)]
/// `Math.log` of V8 13.6.233.17 (fdlibm).
pub fn log(x: f64) -> f64 {
    let mut x = x;
    let mut hx = high_word(x) as i32;
    let lx = low_word(x);

    let mut k: i32 = 0;
    if hx < 0x0010_0000 {
        if (((hx & 0x7FFF_FFFF) as u32) | lx) == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return f64::NAN;
        }
        k -= 54;
        x *= TWO54;
        hx = high_word(x) as i32;
    }
    if hx >= 0x7FF0_0000 {
        return x + x;
    }
    k += (hx >> 20) - 1023;
    hx &= 0x000F_FFFF;
    let mut i = (hx + 0x95F64) & 0x10_0000;
    x = set_high_word(x, (hx | (i ^ 0x3FF0_0000)) as u32);
    k += i >> 20;
    let f = x - 1.0;
    if (0x000F_FFFF & (2 + hx)) < 3 {
        if f == 0.0 {
            if k == 0 {
                return 0.0;
            }
            let dk = f64::from(k);
            return dk * LN2HI0 + dk * LN2LO0;
        }
        let r = f * f * (0.5 - 0.33333333333333333 * f);
        if k == 0 {
            return f - r;
        }
        let dk = f64::from(k);
        return dk * LN2HI0 - ((r - dk * LN2LO0) - f);
    }
    let s = f / (2.0 + f);
    let dk = f64::from(k);
    let z = s * s;
    i = hx - 0x6147A;
    let w = z * z;
    let j = 0x6B851 - hx;
    let t1 = w * (LG2 + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    i |= j;
    let r = t2 + t1;
    if i > 0 {
        let hfsq = 0.5 * f * f;
        if k == 0 { f - (hfsq - s * (hfsq + r)) } else { dk * LN2HI0 - ((hfsq - (s * (hfsq + r) + dk * LN2LO0)) - f) }
    } else if k == 0 {
        f - s * (f - r)
    } else {
        dk * LN2HI0 - ((s * (f - r) - dk * LN2LO0) - f)
    }
}

#[allow(clippy::excessive_precision)]
/// `Math.log1p` of V8 13.6.233.17 (fdlibm).
pub fn log1p(x: f64) -> f64 {
    let hx = high_word(x) as i32;
    let ax = hx & 0x7FFF_FFFF;

    let mut k: i32 = 1;
    let mut f = 0.0;
    let mut hu: i32 = 0;
    let mut c = 0.0;
    if hx < 0x3FDA_827A {
        if ax >= 0x3FF0_0000 {
            if x == -1.0 {
                return f64::NEG_INFINITY;
            }
            return f64::NAN;
        }
        if ax < 0x3E20_0000 {
            if TWO54 + x > 0.0 && ax < 0x3C90_0000 {
                return x;
            }
            return x - x * x * 0.5;
        }
        if hx > 0 || hx <= 0xBFD2_BEC4u32 as i32 {
            k = 0;
            f = x;
            hu = 1;
        }
    }
    if hx >= 0x7FF0_0000 {
        return x + x;
    }
    if k != 0 {
        let mut u;
        if hx < 0x4340_0000 {
            u = 1.0 + x;
            hu = high_word(u) as i32;
            k = (hu >> 20) - 1023;
            c = if k > 0 { 1.0 - (u - x) } else { x - (u - 1.0) };
            c /= u;
        } else {
            u = x;
            hu = high_word(u) as i32;
            k = (hu >> 20) - 1023;
            c = 0.0;
        }
        hu &= 0x000F_FFFF;
        if hu < 0x6A09E {
            u = set_high_word(u, (hu | 0x3FF0_0000) as u32);
        } else {
            k += 1;
            u = set_high_word(u, (hu | 0x3FE0_0000) as u32);
            hu = (0x0010_0000 - hu) >> 2;
        }
        f = u - 1.0;
    }
    let hfsq = 0.5 * f * f;
    if hu == 0 {
        if f == 0.0 {
            if k == 0 {
                return 0.0;
            }
            c += f64::from(k) * LN2LO0;
            return f64::from(k) * LN2HI0 + c;
        }
        let r = hfsq * (1.0 - 0.66666666666666666 * f);
        if k == 0 {
            return f - r;
        }
        return f64::from(k) * LN2HI0 - ((r - (f64::from(k) * LN2LO0 + c)) - f);
    }
    let s = f / (2.0 + f);
    let z = s * s;
    let r = z * (LG1 + z * (LG2 + z * (LG3 + z * (LG4 + z * (LG5 + z * (LG6 + z * LG7))))));
    if k == 0 { f - (hfsq - s * (hfsq + r)) } else { f64::from(k) * LN2HI0 - ((hfsq - (s * (hfsq + r) + (f64::from(k) * LN2LO0 + c))) - f) }
}

fn k_log1p(f: f64) -> f64 {
    let s = f / (2.0 + f);
    let z = s * s;
    let w = z * z;
    let t1 = w * (LG2 + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    let r = t2 + t1;
    let hfsq = 0.5 * f * f;
    s * (hfsq + r)
}

/// `Math.log2` of V8 13.6.233.17 (fdlibm).
pub fn log2(x: f64) -> f64 {
    let mut x = x;
    let mut hx = high_word(x) as i32;
    let lx = low_word(x);

    let mut k: i32 = 0;
    if hx < 0x0010_0000 {
        if (((hx & 0x7FFF_FFFF) as u32) | lx) == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return f64::NAN;
        }
        k -= 54;
        x *= TWO54;
        hx = high_word(x) as i32;
    }
    if hx >= 0x7FF0_0000 {
        return x + x;
    }
    if hx == 0x3FF0_0000 && lx == 0 {
        return 0.0;
    }
    k += (hx >> 20) - 1023;
    hx &= 0x000F_FFFF;
    let i = (hx + 0x95F64) & 0x10_0000;
    x = set_high_word(x, (hx | (i ^ 0x3FF0_0000)) as u32);
    k += i >> 20;
    let y = f64::from(k);
    let f = x - 1.0;
    let hfsq = 0.5 * f * f;
    let r = k_log1p(f);

    let mut hi = f - hfsq;
    hi = set_low_word(hi, 0);
    let lo = (f - hi) - hfsq + r;
    let mut val_hi = hi * IVLN2HI;
    let mut val_lo = (lo + hi) * IVLN2LO + lo * IVLN2HI;

    let w = y + val_hi;
    val_lo += (y - w) + val_hi;
    val_hi = w;

    val_lo + val_hi
}

/// `Math.log10` of V8 13.6.233.17 (fdlibm).
pub fn log10(x: f64) -> f64 {
    let mut x = x;
    let mut hx = high_word(x) as i32;
    let mut lx = low_word(x);

    let mut k: i32 = 0;
    if hx < 0x0010_0000 {
        if (((hx & 0x7FFF_FFFF) as u32) | lx) == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return f64::NAN;
        }
        k -= 54;
        x *= TWO54;
        hx = high_word(x) as i32;
        lx = low_word(x);
    }
    if hx >= 0x7FF0_0000 {
        return x + x;
    }
    if hx == 0x3FF0_0000 && lx == 0 {
        return 0.0;
    }
    k += (hx >> 20) - 1023;

    let i = ((k as u32 & 0x8000_0000) >> 31) as i32;
    hx = (hx & 0x000F_FFFF) | ((0x3FF - i) << 20);
    let y = f64::from(k + i);
    x = set_high_word(x, hx as u32);
    x = set_low_word(x, lx);

    let z = y * LOG10_2LO + IVLN10 * log(x);
    z + y * LOG10_2HI
}

/// `Math.expm1` of V8 13.6.233.17 (fdlibm).
pub fn expm1(x: f64) -> f64 {
    let mut x = x;
    let mut c = 0.0;
    let k: i32;

    let mut hx = high_word(x);
    let xsb = hx & 0x8000_0000;
    hx &= 0x7FFF_FFFF;

    if hx >= 0x4043_687A {
        if hx >= 0x4086_2E42 {
            if hx >= 0x7FF0_0000 {
                let low = low_word(x);
                if ((hx & 0xF_FFFF) | low) != 0 {
                    return x + x;
                }
                return if xsb == 0 { x } else { -1.0 };
            }
            if x > O_THRESHOLD {
                return HUGE * HUGE;
            }
        }
        if xsb != 0 && x + TINY < 0.0 {
            return TINY - 1.0;
        }
    }

    if hx > 0x3FD6_2E42 {
        let (hi, lo);
        if hx < 0x3FF0_A2B2 {
            if xsb == 0 {
                hi = x - LN2HI0;
                lo = LN2LO0;
                k = 1;
            } else {
                hi = x + LN2HI0;
                lo = -LN2LO0;
                k = -1;
            }
        } else {
            k = (INVLN2 * x + if xsb == 0 { 0.5 } else { -0.5 }) as i32;
            let t = f64::from(k);
            hi = x - t * LN2HI0;
            lo = t * LN2LO0;
        }
        x = hi - lo;
        c = (hi - x) - lo;
    } else if hx < 0x3C90_0000 {
        let t = HUGE + x;
        return x - (t - (HUGE + x));
    } else {
        k = 0;
    }

    let hfx = 0.5 * x;
    let hxs = x * hfx;
    let r1 = 1.0 + hxs * (Q1 + hxs * (Q2 + hxs * (Q3 + hxs * (Q4 + hxs * Q5))));
    let mut t = 3.0 - r1 * hfx;
    let mut e = hxs * ((r1 - t) / (6.0 - x * t));
    if k == 0 {
        return x - (x * e - hxs);
    }
    let twopk = from_words(0x3FF0_0000u32.wrapping_add(((k as u32) << 20) as i32 as u32), 0);
    e = x * (e - c) - c;
    e -= hxs;
    if k == -1 {
        return 0.5 * (x - e) - 0.5;
    }
    if k == 1 {
        if x < -0.25 {
            return -2.0 * (e - (x + 0.5));
        }
        return 1.0 + 2.0 * (x - e);
    }
    let mut y;
    if !(-1..=56).contains(&k) {
        y = 1.0 - (e - x);
        if k == 1024 {
            y = y * 2.0 * 8.98846567431158e+307;
        } else {
            y *= twopk;
        }
        return y - 1.0;
    }
    t = 1.0;
    if k < 20 {
        t = set_high_word(t, (0x3FF0_0000 - (0x20_0000 >> k)) as u32);
        y = t - (e - x);
        y *= twopk;
    } else {
        t = set_high_word(t, ((0x3FF - k) << 20) as u32);
        y = x - (e + t);
        y += 1.0;
        y *= twopk;
    }
    y
}
