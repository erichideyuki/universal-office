//! `acos` (ieee754.cc:862), `asin` (991), `atan` (1116), `atan2` (1228), `cos` (1352), `sin`
//! (2448) and `tan` (2510) of V8's fdlibm (the non-glibc branches).

use super::kernels::{kernel_cos, kernel_sin, kernel_tan};
use super::rem_pio2::rem_pio2;
use super::{consts, high_word, low_word, set_low_word};

consts! { checks;
    PI = 0x400921FB, 0x54442D18, 3.14159265358979311600e+00;
    PIO2_HI = 0x3FF921FB, 0x54442D18, 1.57079632679489655800e+00;
    PIO2_LO = 0x3C91A626, 0x33145C07, 6.12323399573676603587e-17;
    PIO4_HI = 0x3FE921FB, 0x54442D18, 7.85398163397448278999e-01;
    PS0 = 0x3FC55555, 0x55555555, 1.66666666666666657415e-01;
    PS1 = 0xBFD4D612, 0x03EB6F7D, -3.25565818622400915405e-01;
    PS2 = 0x3FC9C155, 0x0E884455, 2.01212532134862925881e-01;
    PS3 = 0xBFA48228, 0xB5688F3B, -4.00555345006794114027e-02;
    PS4 = 0x3F49EFE0, 0x7501B288, 7.91534994289814532176e-04;
    PS5 = 0x3F023DE1, 0x0DFDF709, 3.47933107596021167570e-05;
    QS1 = 0xC0033A27, 0x1C8A2D4B, -2.40339491173441421878e+00;
    QS2 = 0x40002AE5, 0x9C598AC8, 2.02094576023350569471e+00;
    QS3 = 0xBFE6066C, 0x1B8D0159, -6.88283971605453293030e-01;
    QS4 = 0x3FB3B8C5, 0xB12E9282, 7.70381505559019352791e-02;
    ATANHI0 = 0x3FDDAC67, 0x0561BB4F, 4.63647609000806093515e-01;
    ATANHI1 = 0x3FE921FB, 0x54442D18, 7.85398163397448278999e-01;
    ATANHI2 = 0x3FEF730B, 0xD281F69B, 9.82793723247329054082e-01;
    ATANHI3 = 0x3FF921FB, 0x54442D18, 1.57079632679489655800e+00;
    ATANLO0 = 0x3C7A2B7F, 0x222F65E2, 2.26987774529616870924e-17;
    ATANLO1 = 0x3C81A626, 0x33145C07, 3.06161699786838301793e-17;
    ATANLO2 = 0x3C700788, 0x7AF0CBBD, 1.39033110312309984516e-17;
    ATANLO3 = 0x3C91A626, 0x33145C07, 6.12323399573676603587e-17;
    AT0 = 0x3FD55555, 0x5555550D, 3.33333333333329318027e-01;
    AT1 = 0xBFC99999, 0x9998EBC4, -1.99999999998764832476e-01;
    AT2 = 0x3FC24924, 0x920083FF, 1.42857142725034663711e-01;
    AT3 = 0xBFBC71C6, 0xFE231671, -1.11111104054623557880e-01;
    AT4 = 0x3FB745CD, 0xC54C206E, 9.09088713343650656196e-02;
    AT5 = 0xBFB3B0F2, 0xAF749A6D, -7.69187620504482999495e-02;
    AT6 = 0x3FB10D66, 0xA0D03D51, 6.66107313738753120669e-02;
    AT7 = 0xBFADDE2D, 0x52DEFD9A, -5.83357013379057348645e-02;
    AT8 = 0x3FA97B4B, 0x24760DEB, 4.97687799461593236017e-02;
    AT9 = 0xBFA2B444, 0x2C6A6C2F, -3.65315727442169155270e-02;
    AT10 = 0x3F90AD3A, 0xE322DA11, 1.62858201153657823623e-02;
    PI_O_4 = 0x3FE921FB, 0x54442D18, 7.8539816339744827900E-01;
    PI_O_2 = 0x3FF921FB, 0x54442D18, 1.5707963267948965580E+00;
    PI_LO = 0x3CA1A626, 0x33145C07, 1.2246467991473531772E-16;
}

const HUGE: f64 = 1.0e300;
const TINY: f64 = 1.0e-300;
const ATANHI: [f64; 4] = [ATANHI0, ATANHI1, ATANHI2, ATANHI3];
const ATANLO: [f64; 4] = [ATANLO0, ATANLO1, ATANLO2, ATANLO3];
const AT: [f64; 11] = [AT0, AT1, AT2, AT3, AT4, AT5, AT6, AT7, AT8, AT9, AT10];

/// `Math.acos` of V8 13.6.233.17 (fdlibm).
pub fn acos(x: f64) -> f64 {
    let hx = high_word(x) as i32;
    let ix = hx & 0x7FFF_FFFF;
    if ix >= 0x3FF0_0000 {
        let lx = low_word(x);
        if (((ix - 0x3FF0_0000) as u32) | lx) == 0 {
            if hx > 0 {
                return 0.0;
            }
            return PI + 2.0 * PIO2_LO;
        }
        return f64::NAN;
    }
    if ix < 0x3FE0_0000 {
        if ix <= 0x3C60_0000 {
            return PIO2_HI + PIO2_LO;
        }
        let z = x * x;
        let p = z * (PS0 + z * (PS1 + z * (PS2 + z * (PS3 + z * (PS4 + z * PS5)))));
        let q = 1.0 + z * (QS1 + z * (QS2 + z * (QS3 + z * QS4)));
        let r = p / q;
        PIO2_HI - (x - (PIO2_LO - x * r))
    } else if hx < 0 {
        let z = (1.0 + x) * 0.5;
        let p = z * (PS0 + z * (PS1 + z * (PS2 + z * (PS3 + z * (PS4 + z * PS5)))));
        let q = 1.0 + z * (QS1 + z * (QS2 + z * (QS3 + z * QS4)));
        let s = z.sqrt();
        let r = p / q;
        let w = r * s - PIO2_LO;
        PI - 2.0 * (s + w)
    } else {
        let z = (1.0 - x) * 0.5;
        let s = z.sqrt();
        let df = set_low_word(s, 0);
        let c = (z - df * df) / (s + df);
        let p = z * (PS0 + z * (PS1 + z * (PS2 + z * (PS3 + z * (PS4 + z * PS5)))));
        let q = 1.0 + z * (QS1 + z * (QS2 + z * (QS3 + z * QS4)));
        let r = p / q;
        let w = r * s + c;
        2.0 * (df + w)
    }
}

/// `Math.asin` of V8 13.6.233.17 (fdlibm).
pub fn asin(x: f64) -> f64 {
    let hx = high_word(x) as i32;
    let ix = hx & 0x7FFF_FFFF;
    let mut t = 0.0;
    if ix >= 0x3FF0_0000 {
        let lx = low_word(x);
        if (((ix - 0x3FF0_0000) as u32) | lx) == 0 {
            return x * PIO2_HI + x * PIO2_LO;
        }
        return f64::NAN;
    } else if ix < 0x3FE0_0000 {
        if ix < 0x3E40_0000 {
            if HUGE + x > 1.0 {
                return x;
            }
        } else {
            t = x * x;
        }
        let p = t * (PS0 + t * (PS1 + t * (PS2 + t * (PS3 + t * (PS4 + t * PS5)))));
        let q = 1.0 + t * (QS1 + t * (QS2 + t * (QS3 + t * QS4)));
        let w = p / q;
        return x + x * w;
    }
    let mut w = 1.0 - x.abs();
    t = w * 0.5;
    let mut p = t * (PS0 + t * (PS1 + t * (PS2 + t * (PS3 + t * (PS4 + t * PS5)))));
    let mut q = 1.0 + t * (QS1 + t * (QS2 + t * (QS3 + t * QS4)));
    let s = t.sqrt();
    if ix >= 0x3FEF_3333 {
        w = p / q;
        t = PIO2_HI - (2.0 * (s + s * w) - PIO2_LO);
    } else {
        w = set_low_word(s, 0);
        let c = (t - w * w) / (s + w);
        let r = p / q;
        p = 2.0 * s * r - (PIO2_LO - 2.0 * c);
        q = PIO4_HI - 2.0 * w;
        t = PIO4_HI - (p - q);
    }
    if hx > 0 { t } else { -t }
}

/// `Math.atan` of V8 13.6.233.17 (fdlibm).
pub fn atan(x: f64) -> f64 {
    let mut x = x;
    let hx = high_word(x) as i32;
    let ix = hx & 0x7FFF_FFFF;
    if ix >= 0x4410_0000 {
        let low = low_word(x);
        if ix > 0x7FF0_0000 || (ix == 0x7FF0_0000 && low != 0) {
            return x + x;
        }
        if hx > 0 {
            return ATANHI[3] + ATANLO[3];
        }
        return -ATANHI[3] - ATANLO[3];
    }
    let id: i32;
    if ix < 0x3FDC_0000 {
        if ix < 0x3E40_0000 && HUGE + x > 1.0 {
            return x;
        }
        id = -1;
    } else {
        x = x.abs();
        if ix < 0x3FF3_0000 {
            if ix < 0x3FE6_0000 {
                id = 0;
                x = (2.0 * x - 1.0) / (2.0 + x);
            } else {
                id = 1;
                x = (x - 1.0) / (x + 1.0);
            }
        } else if ix < 0x4003_8000 {
            id = 2;
            x = (x - 1.5) / (1.0 + 1.5 * x);
        } else {
            id = 3;
            x = -1.0 / x;
        }
    }
    let z = x * x;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));
    if id < 0 {
        x - x * (s1 + s2)
    } else {
        let id = id as usize;
        let z = ATANHI[id] - ((x * (s1 + s2) - ATANLO[id]) - x);
        if hx < 0 { -z } else { z }
    }
}

/// `Math.atan2` of V8 13.6.233.17 (fdlibm).
pub fn atan2(y: f64, x: f64) -> f64 {
    let hx = high_word(x) as i32;
    let lx = low_word(x);
    let ix = hx & 0x7FFF_FFFF;
    let hy = high_word(y) as i32;
    let ly = low_word(y);
    let iy = hy & 0x7FFF_FFFF;
    if (((ix as u32) | ((lx | lx.wrapping_neg()) >> 31)) > 0x7FF0_0000) || (((iy as u32) | ((ly | ly.wrapping_neg()) >> 31)) > 0x7FF0_0000)
    {
        return x + y;
    }
    if ((hx.wrapping_sub(0x3FF0_0000) as u32) | lx) == 0 {
        return atan(y);
    }
    let mut m = ((hy >> 31) & 1) | ((hx >> 30) & 2);

    if ((iy as u32) | ly) == 0 {
        match m {
            0 | 1 => return y,
            2 => return PI + TINY,
            _ => return -PI - TINY,
        }
    }
    if ((ix as u32) | lx) == 0 {
        return if hy < 0 { -PI_O_2 - TINY } else { PI_O_2 + TINY };
    }

    if ix == 0x7FF0_0000 {
        if iy == 0x7FF0_0000 {
            match m {
                0 => return PI_O_4 + TINY,
                1 => return -PI_O_4 - TINY,
                2 => return 3.0 * PI_O_4 + TINY,
                _ => return -3.0 * PI_O_4 - TINY,
            }
        } else {
            match m {
                0 => return 0.0,
                1 => return -0.0,
                2 => return PI + TINY,
                _ => return -PI - TINY,
            }
        }
    }
    if iy == 0x7FF0_0000 {
        return if hy < 0 { -PI_O_2 - TINY } else { PI_O_2 + TINY };
    }

    let k = (iy - ix) >> 20;
    let z;
    if k > 60 {
        z = PI_O_2 + 0.5 * PI_LO;
        m &= 1;
    } else if hx < 0 && k < -60 {
        z = 0.0;
    } else {
        z = atan((y / x).abs());
    }
    match m {
        0 => z,
        1 => -z,
        2 => PI - (z - PI_LO),
        _ => (z - PI_LO) - PI,
    }
}

/// `Math.cos` of V8 13.6.233.17 (fdlibm).
pub fn cos(x: f64) -> f64 {
    let ix = (high_word(x) as i32) & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        kernel_cos(x, 0.0)
    } else if ix >= 0x7FF0_0000 {
        x - x
    } else {
        let (n, y0, y1) = rem_pio2(x);
        match n & 3 {
            0 => kernel_cos(y0, y1),
            1 => -kernel_sin(y0, y1, 1),
            2 => -kernel_cos(y0, y1),
            _ => kernel_sin(y0, y1, 1),
        }
    }
}

/// `Math.sin` of V8 13.6.233.17 (fdlibm).
pub fn sin(x: f64) -> f64 {
    let ix = (high_word(x) as i32) & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        kernel_sin(x, 0.0, 0)
    } else if ix >= 0x7FF0_0000 {
        x - x
    } else {
        let (n, y0, y1) = rem_pio2(x);
        match n & 3 {
            0 => kernel_sin(y0, y1, 1),
            1 => kernel_cos(y0, y1),
            2 => -kernel_sin(y0, y1, 1),
            _ => -kernel_cos(y0, y1),
        }
    }
}

/// `Math.tan` of V8 13.6.233.17 (fdlibm).
pub fn tan(x: f64) -> f64 {
    let ix = (high_word(x) as i32) & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        kernel_tan(x, 0.0, 1)
    } else if ix >= 0x7FF0_0000 {
        x - x
    } else {
        let (n, y0, y1) = rem_pio2(x);
        kernel_tan(y0, y1, 1 - ((n & 1) << 1))
    }
}
