//! `__kernel_cos` (ieee754.cc:305, the fdlibm 5.3 `qx` variant), `__kernel_sin` (673) and
//! `__kernel_tan` (733).

use super::{consts, from_words, high_word, low_word, set_low_word};

consts! { checks;
    C1 = 0x3FA55555, 0x5555554C, 4.16666666666666019037e-02;
    C2 = 0xBF56C16C, 0x16C15177, -1.38888888888741095749e-03;
    C3 = 0x3EFA01A0, 0x19CB1590, 2.48015872894767294178e-05;
    C4 = 0xBE927E4F, 0x809C52AD, -2.75573143513906633035e-07;
    C5 = 0x3E21EE9E, 0xBDB4B1C4, 2.08757232129817482790e-09;
    C6 = 0xBDA8FAE9, 0xBE8838D4, -1.13596475577881948265e-11;
    S1 = 0xBFC55555, 0x55555549, -1.66666666666666324348e-01;
    S2 = 0x3F811111, 0x1110F8A6, 8.33333333332248946124e-03;
    S3 = 0xBF2A01A0, 0x19C161D5, -1.98412698298579493134e-04;
    S4 = 0x3EC71DE3, 0x57B1FE7D, 2.75573137070700676789e-06;
    S5 = 0xBE5AE5E6, 0x8A2B9CEB, -2.50507602534068634195e-08;
    S6 = 0x3DE5D93A, 0x5ACFD57C, 1.58969099521155010221e-10;
    T0 = 0x3FD55555, 0x55555563, 3.33333333333334091986e-01;
    T1 = 0x3FC11111, 0x1110FE7A, 1.33333333333201242699e-01;
    T2 = 0x3FABA1BA, 0x1BB341FE, 5.39682539762260521377e-02;
    T3 = 0x3F9664F4, 0x8406D637, 2.18694882948595424599e-02;
    T4 = 0x3F8226E3, 0xE96E8493, 8.86323982359930005737e-03;
    T5 = 0x3F6D6D22, 0xC9560328, 3.59207910759131235356e-03;
    T6 = 0x3F57DBC8, 0xFEE08315, 1.45620945432529025516e-03;
    T7 = 0x3F4344D8, 0xF2F26501, 5.88041240820264096874e-04;
    T8 = 0x3F3026F7, 0x1A8D1068, 2.46463134818469906812e-04;
    T9 = 0x3F147E88, 0xA03792A6, 7.81794442939557092300e-05;
    T10 = 0x3F12B80F, 0x32F0A7E9, 7.14072491382608190305e-05;
    T11 = 0xBEF375CB, 0xDB605373, -1.85586374855275456654e-05;
    T12 = 0x3EFB2A70, 0x74BF7AD4, 2.59073051863633712884e-05;
    PIO4 = 0x3FE921FB, 0x54442D18, 7.85398163397448278999e-01;
    PIO4LO = 0x3C81A626, 0x33145C07, 3.06161699786838301793e-17;
}

const T: [f64; 13] = [T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12];

pub(crate) fn kernel_cos(x: f64, y: f64) -> f64 {
    let ix = (high_word(x) as i32) & 0x7FFF_FFFF;
    if ix < 0x3E40_0000 && x as i32 == 0 {
        return 1.0;
    }
    let z = x * x;
    let r = z * (C1 + z * (C2 + z * (C3 + z * (C4 + z * (C5 + z * C6)))));
    if ix < 0x3FD3_3333 {
        1.0 - (0.5 * z - (z * r - x * y))
    } else {
        let qx = if ix > 0x3FE9_0000 { 0.28125 } else { from_words((ix - 0x0020_0000) as u32, 0) };
        let iz = 0.5 * z - qx;
        let a = 1.0 - qx;
        a - (iz - (z * r - x * y))
    }
}

pub(crate) fn kernel_sin(x: f64, y: f64, iy: i32) -> f64 {
    let ix = (high_word(x) as i32) & 0x7FFF_FFFF;
    if ix < 0x3E40_0000 && x as i32 == 0 {
        return x;
    }
    let z = x * x;
    let v = z * x;
    let r = S2 + z * (S3 + z * (S4 + z * (S5 + z * S6)));
    if iy == 0 { x + v * (S1 + z * r) } else { x - ((z * (0.5 * y - v * r) - y) - v * S1) }
}

pub(crate) fn kernel_tan(x: f64, y: f64, iy: i32) -> f64 {
    let mut x = x;
    let mut y = y;
    let hx = high_word(x) as i32;
    let ix = hx & 0x7FFF_FFFF;
    if ix < 0x3E30_0000 && x as i32 == 0 {
        let low = low_word(x);
        if (((ix as u32) | low) | ((iy + 1) as u32)) == 0 {
            return 1.0 / x.abs();
        } else if iy == 1 {
            return x;
        } else {
            let mut z;
            let w = x + y;
            z = w;
            z = set_low_word(z, 0);
            let v = y - (z - x);
            let a = -1.0 / w;
            let mut t = a;
            t = set_low_word(t, 0);
            let s = 1.0 + t * z;
            return t + a * (s + t * v);
        }
    }
    if ix >= 0x3FE5_9428 {
        if hx < 0 {
            x = -x;
            y = -y;
        }
        let z = PIO4 - x;
        let w = PIO4LO - y;
        x = z + w;
        y = 0.0;
    }
    let z = x * x;
    let mut w = z * z;
    let mut r = T[1] + w * (T[3] + w * (T[5] + w * (T[7] + w * (T[9] + w * T[11]))));
    let mut v = z * (T[2] + w * (T[4] + w * (T[6] + w * (T[8] + w * (T[10] + w * T[12])))));
    let mut s = z * x;
    r = y + z * (s * (r + v) + y);
    r += T[0] * s;
    w = x + r;
    if ix >= 0x3FE5_9428 {
        v = f64::from(iy);
        return f64::from(1 - ((hx >> 30) & 2)) * (v - 2.0 * (x - (w * w / (w + v) - r)));
    }
    if iy == 1 {
        w
    } else {
        let mut z = w;
        z = set_low_word(z, 0);
        v = r - (z - x);
        let a = -1.0 / w;
        let mut t = a;
        t = set_low_word(t, 0);
        s = 1.0 + t * z;
        t + a * (s + t * v)
    }
}
