//! `__ieee754_rem_pio2` (ieee754.cc:118) and `__kernel_rem_pio2` (ieee754.cc:443): argument
//! reduction modulo pi/2.

use super::{consts, high_word, low_word, scalbn, set_high_word, set_low_word};

/// 396 hex digits of 2/pi.
const TWO_OVER_PI: [i32; 66] = [
    0xA2F983, 0x6E4E44, 0x1529FC, 0x2757D1, 0xF534DD, 0xC0DB62, 0x95993C, 0x439041, 0xFE5163, 0xABDEBB, 0xC561B7, 0x246E3A, 0x424DD2,
    0xE00649, 0x2EEA09, 0xD1921C, 0xFE1DEB, 0x1CB129, 0xA73EE8, 0x8235F5, 0x2EBB44, 0x84E99C, 0x7026B4, 0x5F7E41, 0x3991D6, 0x398353,
    0x39F49C, 0x845F8B, 0xBDF928, 0x3B1FF8, 0x97FFDE, 0x05980F, 0xEF2F11, 0x8B5A0A, 0x6D1F6D, 0x367ECF, 0x27CB09, 0xB74F46, 0x3F669E,
    0x5FEA2D, 0x7527BA, 0xC7EBE5, 0xF17B3D, 0x0739F7, 0x8A5292, 0xEA6BFB, 0x5FB11F, 0x8D5D08, 0x560330, 0x46FC7B, 0x6BABF0, 0xCFBC20,
    0x9AF436, 0x1DA9E3, 0x91615E, 0xE61B08, 0x659985, 0x5F14A0, 0x68408D, 0xFFD880, 0x4D7327, 0x310606, 0x1556CA, 0x73A8C9, 0x60E27B,
    0xC08C6B,
];

const NPIO2_HW: [i32; 32] = [
    0x3FF921FB, 0x400921FB, 0x4012D97C, 0x401921FB, 0x401F6A7A, 0x4022D97C, 0x4025FDBB, 0x402921FB, 0x402C463A, 0x402F6A7A, 0x4031475C,
    0x4032D97C, 0x40346B9C, 0x4035FDBB, 0x40378FDB, 0x403921FB, 0x403AB41B, 0x403C463A, 0x403DD85A, 0x403F6A7A, 0x40407E4C, 0x4041475C,
    0x4042106C, 0x4042D97C, 0x4043A28C, 0x40446B9C, 0x404534AC, 0x4045FDBB, 0x4046C6CB, 0x40478FDB, 0x404858EB, 0x404921FB,
];

consts! { checks;
    HALF = 0x3FE00000, 0x00000000, 5.00000000000000000000e-01;
    TWO24 = 0x41700000, 0x00000000, 1.67772160000000000000e+07;
    INVPIO2 = 0x3FE45F30, 0x6DC9C883, 6.36619772367581382433e-01;
    PIO2_1 = 0x3FF921FB, 0x54400000, 1.57079632673412561417e+00;
    PIO2_1T = 0x3DD0B461, 0x1A626331, 6.07710050650619224932e-11;
    PIO2_2 = 0x3DD0B461, 0x1A600000, 6.07710050630396597660e-11;
    PIO2_2T = 0x3BA3198A, 0x2E037073, 2.02226624879595063154e-21;
    PIO2_3 = 0x3BA3198A, 0x2E000000, 2.02226624871116645580e-21;
    PIO2_3T = 0x397B839A, 0x252049C1, 8.47842766036889956997e-32;
    PIO2_0 = 0x3FF921FB, 0x40000000, 1.57079625129699707031e+00;
    PIO2_K1 = 0x3E74442D, 0x00000000, 7.54978941586159635335e-08;
    PIO2_K2 = 0x3CF84698, 0x80000000, 5.39030252995776476554e-15;
    PIO2_K3 = 0x3B78CC51, 0x60000000, 3.28200341580791294123e-22;
    PIO2_K4 = 0x39F01B83, 0x80000000, 1.27065575308067607349e-29;
    PIO2_K5 = 0x387A2520, 0x40000000, 1.22933308981111328932e-36;
    PIO2_K6 = 0x36E38222, 0x80000000, 2.73370053816464559624e-44;
    PIO2_K7 = 0x3569F31D, 0x00000000, 2.16741683877804819444e-51;
    TWON24 = 0x3E700000, 0x00000000, 5.96046447753906250000e-08;
}

/// `__ieee754_rem_pio2(x, y)`: returns `n`, `y[0]` and `y[1]` with x = n·pi/2 + y[0] + y[1].
pub(crate) fn rem_pio2(x: f64) -> (i32, f64, f64) {
    let hx = high_word(x) as i32;
    let ix = hx & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        return (0, x, 0.0);
    }
    if ix < 0x4002_D97C {
        if hx > 0 {
            let mut z = x - PIO2_1;
            let (y0, y1);
            if ix != 0x3FF9_21FB {
                y0 = z - PIO2_1T;
                y1 = (z - y0) - PIO2_1T;
            } else {
                z -= PIO2_2;
                y0 = z - PIO2_2T;
                y1 = (z - y0) - PIO2_2T;
            }
            return (1, y0, y1);
        }
        let mut z = x + PIO2_1;
        let (y0, y1);
        if ix != 0x3FF9_21FB {
            y0 = z + PIO2_1T;
            y1 = (z - y0) + PIO2_1T;
        } else {
            z += PIO2_2;
            y0 = z + PIO2_2T;
            y1 = (z - y0) + PIO2_2T;
        }
        return (-1, y0, y1);
    }
    if ix <= 0x4139_21FB {
        let mut t = x.abs();
        let n = (t * INVPIO2 + HALF) as i32;
        let fn_ = f64::from(n);
        let mut r = t - fn_ * PIO2_1;
        let mut w = fn_ * PIO2_1T;
        let mut y0;
        if n < 32 && ix != NPIO2_HW[(n - 1) as usize] {
            y0 = r - w;
        } else {
            let j = ix >> 20;
            y0 = r - w;
            let mut high = high_word(y0);
            let mut i = j - ((high >> 20) & 0x7FF) as i32;
            if i > 16 {
                t = r;
                w = fn_ * PIO2_2;
                r = t - w;
                w = fn_ * PIO2_2T - ((t - r) - w);
                y0 = r - w;
                high = high_word(y0);
                i = j - ((high >> 20) & 0x7FF) as i32;
                if i > 49 {
                    t = r;
                    w = fn_ * PIO2_3;
                    r = t - w;
                    w = fn_ * PIO2_3T - ((t - r) - w);
                    y0 = r - w;
                }
            }
        }
        let y1 = (r - y0) - w;
        if hx < 0 {
            return (-n, -y0, -y1);
        }
        return (n, y0, y1);
    }
    if ix >= 0x7FF0_0000 {
        let v = x - x;
        return (0, v, v);
    }
    let low = low_word(x);
    let mut z = set_low_word(0.0, low);
    let e0 = (ix >> 20) - 1046;
    z = set_high_word(z, ix.wrapping_sub(((e0 as u32) << 20) as i32) as u32);
    let mut tx = [0.0f64; 3];
    for t in tx.iter_mut().take(2) {
        *t = f64::from(z as i32);
        z = (z - *t) * TWO24;
    }
    tx[2] = z;
    let mut nx = 3usize;
    while tx[nx - 1] == 0.0 {
        nx -= 1;
    }
    let (n, y0, y1) = kernel_rem_pio2(&tx, e0, nx as i32);
    if hx < 0 {
        return (-n, -y0, -y1);
    }
    (n, y0, y1)
}

/// `__kernel_rem_pio2(x, y, e0, nx, prec = 2, two_over_pi)`: the only `prec` the callers use
/// (jk = init_jk[2] = 4, the `y[0]`/`y[1]` branch of the final switch); returns `n & 7`, `y[0]`
/// and `y[1]`.
fn kernel_rem_pio2(x: &[f64; 3], e0: i32, nx: i32) -> (i32, f64, f64) {
    const PIO2: [f64; 8] = [PIO2_0, PIO2_K1, PIO2_K2, PIO2_K3, PIO2_K4, PIO2_K5, PIO2_K6, PIO2_K7];
    let ipio2 = &TWO_OVER_PI;
    let jk: i32 = 4;
    let jp = jk;
    let mut iq = [0i32; 20];
    let mut f = [0.0f64; 20];
    let mut fq = [0.0f64; 20];
    let mut q = [0.0f64; 20];

    let jx = nx - 1;
    let mut jv = (e0 - 3) / 24;
    if jv < 0 {
        jv = 0;
    }
    let mut q0 = e0 - 24 * (jv + 1);

    let mut j = jv - jx;
    let m = jx + jk;
    for fi in f.iter_mut().take(m as usize + 1) {
        *fi = if j < 0 { 0.0 } else { f64::from(ipio2[j as usize]) };
        j += 1;
    }

    for i in 0..=jk {
        let mut fw = 0.0;
        for j in 0..=jx {
            fw += x[j as usize] * f[(jx + i - j) as usize];
        }
        q[i as usize] = fw;
    }

    let mut jz = jk;
    let mut n;
    let mut z;
    let mut fw;
    let ih;
    loop {
        let mut i = 0usize;
        j = jz;
        z = q[jz as usize];
        while j > 0 {
            fw = f64::from((TWON24 * z) as i32);
            iq[i] = (z - TWO24 * fw) as i32;
            z = q[(j - 1) as usize] + fw;
            i += 1;
            j -= 1;
        }

        z = scalbn(z, q0);
        z -= 8.0 * (z * 0.125).floor();
        n = z as i32;
        z -= f64::from(n);
        let mut ih_ = 0;
        if q0 > 0 {
            let i = iq[(jz - 1) as usize] >> (24 - q0);
            n += i;
            iq[(jz - 1) as usize] -= i << (24 - q0);
            ih_ = iq[(jz - 1) as usize] >> (23 - q0);
        } else if q0 == 0 {
            ih_ = iq[(jz - 1) as usize] >> 23;
        } else if z >= 0.5 {
            ih_ = 2;
        }

        if ih_ > 0 {
            n += 1;
            let mut carry = 0;
            for i in 0..jz as usize {
                let j = iq[i];
                if carry == 0 {
                    if j != 0 {
                        carry = 1;
                        iq[i] = 0x100_0000 - j;
                    }
                } else {
                    iq[i] = 0xFF_FFFF - j;
                }
            }
            if q0 > 0 {
                match q0 {
                    1 => iq[(jz - 1) as usize] &= 0x7F_FFFF,
                    2 => iq[(jz - 1) as usize] &= 0x3F_FFFF,
                    _ => {}
                }
            }
            if ih_ == 2 {
                z = 1.0 - z;
                if carry != 0 {
                    z -= scalbn(1.0, q0);
                }
            }
        }

        if z == 0.0 {
            let mut j = 0;
            let mut i = jz - 1;
            while i >= jk {
                j |= iq[i as usize];
                i -= 1;
            }
            if j == 0 {
                let mut k = 1;
                while jk >= k && iq[(jk - k) as usize] == 0 {
                    k += 1;
                }
                for i in (jz + 1)..=(jz + k) {
                    f[(jx + i) as usize] = f64::from(ipio2[(jv + i) as usize]);
                    let mut fw = 0.0;
                    for j in 0..=jx {
                        fw += x[j as usize] * f[(jx + i - j) as usize];
                    }
                    q[i as usize] = fw;
                }
                jz += k;
                continue;
            }
        }
        ih = ih_;
        break;
    }

    if z == 0.0 {
        jz -= 1;
        q0 -= 24;
        while iq[jz as usize] == 0 {
            jz -= 1;
            q0 -= 24;
        }
    } else {
        z = scalbn(z, -q0);
        if z >= TWO24 {
            fw = f64::from((TWON24 * z) as i32);
            iq[jz as usize] = (z - TWO24 * fw) as i32;
            jz += 1;
            q0 += 24;
            iq[jz as usize] = fw as i32;
        } else {
            iq[jz as usize] = z as i32;
        }
    }

    fw = scalbn(1.0, q0);
    let mut i = jz;
    while i >= 0 {
        q[i as usize] = fw * f64::from(iq[i as usize]);
        fw *= TWON24;
        i -= 1;
    }

    let mut i = jz;
    while i >= 0 {
        fw = 0.0;
        let mut k = 0;
        while k <= jp && k <= jz - i {
            fw += PIO2[k as usize] * q[(i + k) as usize];
            k += 1;
        }
        fq[(jz - i) as usize] = fw;
        i -= 1;
    }

    fw = 0.0;
    let mut i = jz;
    while i >= 0 {
        fw += fq[i as usize];
        i -= 1;
    }
    let y0 = if ih == 0 { fw } else { -fw };
    fw = fq[0] - fw;
    for i in 1..=jz {
        fw += fq[i as usize];
    }
    let y1 = if ih == 0 { fw } else { -fw };
    (n & 7, y0, y1)
}
