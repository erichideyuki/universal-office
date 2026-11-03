//! `cbrt` (ieee754.cc:2333) of V8's fdlibm.

use super::{consts, from_words, high_word, low_word, set_high_word};

consts! { checks;
    P0 = 0x3FFE03E6, 0x0F61E692, 1.87595182427177009643;
    P1 = 0xBFFE28E0, 0x92F02420, -1.88497979543377169875;
    P2 = 0x3FF9F160, 0x4A49D6C2, 1.621429720105354466140;
    P3 = 0xBFE844CB, 0xBEE751D9, -0.758397934778766047437;
    P4 = 0x3FC2B000, 0xD4E4EDD7, 0.145996192886612446982;
}

const B1: u32 = 715_094_163;
const B2: u32 = 696_219_795;

/// `Math.cbrt` of V8 13.6.233.17 (fdlibm).
pub fn cbrt(x: f64) -> f64 {
    let mut hx = high_word(x) as i32;
    let low = low_word(x);
    let sign = (hx as u32) & 0x8000_0000;
    hx = (hx as u32 ^ sign) as i32;
    if hx >= 0x7FF0_0000 {
        return x + x;
    }

    let mut t;
    if hx < 0x0010_0000 {
        if ((hx as u32) | low) == 0 {
            return x;
        }
        t = set_high_word(0.0, 0x4350_0000);
        t *= x;
        let high = high_word(t);
        t = from_words(sign | ((high & 0x7FFF_FFFF) / 3 + B2), 0);
    } else {
        t = from_words(sign | ((hx as u32) / 3 + B1), 0);
    }

    let mut r = (t * t) * (t / x);
    t *= (P0 + r * (P1 + r * P2)) + ((r * r) * r) * (P3 + r * P4);

    let mut bits = t.to_bits();
    bits = bits.wrapping_add(0x8000_0000) & 0xFFFF_FFFF_C000_0000;
    t = f64::from_bits(bits);

    let s = t * t;
    r = x / s;
    let w = t + t;
    r = (r - t) / (w + r);
    t + t * r
}
