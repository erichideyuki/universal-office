//! The Excel / Windows "word sort" text order (`CompareString` with `NORM_IGNORECASE`), ported 1:1 from
//! `sheet/packages/calc/src/collation.ts`, which Sheet's operators, SORT, UNIQUE and MATCH use and Write's
//! table sort borrows (`@universal-office/collation`). Upper case equals lower case, hyphen and apostrophe
//! only break ties, accents are a secondary difference, ligatures expand (`ß` = `ss`, `æ` = `ae`), symbols
//! sort before digits and digits before letters, kana width is a third level.
//!
//! The weights are `f64` exactly as the TS computes them (`40 + (cp - 0xa1) / 100` and friends) and the
//! text is UTF-16 (lone surrogates are code points of their own, as in `for (const ch of text)`). The TS
//! keeps two caches (a key per string, a collation element per character); this port has none: a key is
//! computed per call (build a [`CollationKey`] once to sort), which changes no result.

use std::cmp::Ordering;

use crate::case::{CaseMapping, lower_mapping};
use crate::nfc::nfkd;
use crate::utf16::code_points;

const SPACE: f64 = 10.0;
const PUNCT: &[u8] = b"!\"#$%&()*,./:;?@[\\]^_`{|}~";
const CURRENCY: [u32; 5] = [0xA2, 0xA3, 0xA4, 0xA5, 0x20AC];
const MATH: [u32; 8] = [0x2B, 0x3C, 0x3D, 0x3E, 0xB1, 0xD7, 0xF7, 0xAC];
const MISC: [u32; 6] = [0xA7, 0xA9, 0xAE, 0xB0, 0xB5, 0xB6];
const DIGIT0: f64 = 1000.0;
const LETTER_A: f64 = 2000.0;
const OTHER: f64 = 100_000.0;
const CJK: f64 = 400_000.0;
const STROKE: f64 = 15.0;

/// The secondary weights of the combining diacritics (measured order: acute < grave < circumflex < diaeresis
/// < tilde < ring); the others are `20 + (cp - 0x300)`.
const MARK: [(u32, f64); 13] = [
    (0x301, 2.0),
    (0x300, 3.0),
    (0x302, 4.0),
    (0x308, 5.0),
    (0x303, 6.0),
    (0x30A, 7.0),
    (0x327, 8.0),
    (0x30C, 9.0),
    (0x304, 10.0),
    (0x306, 11.0),
    (0x328, 12.0),
    (0x307, 13.0),
    (0x30B, 14.0),
];

/// Characters with their own treatment: (code point, primary elements, secondary of the first).
const SPECIAL: [(u32, &str, f64); 21] = [
    (0xAA, "a", 1.0),
    (0xBA, "o", 1.0),
    (0xF8, "o", STROKE),
    (0xD8, "o", STROKE),
    (0x111, "d", STROKE),
    (0x110, "d", STROKE),
    (0x142, "l", STROKE),
    (0x141, "l", STROKE),
    (0x127, "h", STROKE),
    (0x126, "h", STROKE),
    (0x131, "i", 1.0),
    (0x149, "n", 40.0),
    (0xDF, "ss", 0.0),
    (0x1E9E, "ss", 0.0),
    (0xE6, "ae", 0.0),
    (0xC6, "ae", 0.0),
    (0x153, "oe", 0.0),
    (0x152, "oe", 0.0),
    (0xB9, "1", 1.0),
    (0xB2, "2", 1.0),
    (0xB3, "3", 1.0),
];

fn is_cjk(cp: u32) -> bool {
    (0x3400..=0x9FFF).contains(&cp) || (0xF900..=0xFAFF).contains(&cp) || (0x20000..=0x3FFFF).contains(&cp)
}

fn index_in(set: &[u32], cp: u32) -> Option<usize> {
    set.iter().position(|&c| c == cp)
}

/// The primary weight of a base character (no diacritic).
fn base_primary(cp: u32) -> f64 {
    if cp < 32 || cp == 0x7F {
        return 1.0;
    }
    if cp == 0x20 || cp == 0xA0 || (0x2000..=0x200A).contains(&cp) || cp == 0x3000 {
        return SPACE;
    }
    if (48..=57).contains(&cp) {
        return DIGIT0 + f64::from(cp - 48);
    }
    let (lc, lo_len) = match lower_mapping(cp) {
        CaseMapping::Same => (cp, if cp > 0xFFFF { 2 } else { 1 }),
        CaseMapping::One(m) => (m, if m > 0xFFFF { 2 } else { 1 }),
        CaseMapping::Many(units) => {
            let first = code_points(units).next().unwrap_or(cp);
            (first, units.len())
        }
    };
    if lo_len == 1 && (97..=122).contains(&lc) {
        return LETTER_A + f64::from(lc - 97);
    }
    if let Some(i) = u8::try_from(cp).ok().and_then(|b| PUNCT.iter().position(|&c| c == b)) {
        return 11.0 + i as f64;
    }
    if let Some(i) = index_in(&CURRENCY, cp) {
        return 60.0 + i as f64;
    }
    if let Some(i) = index_in(&MATH, cp) {
        return 70.0 + i as f64;
    }
    if let Some(i) = index_in(&MISC, cp) {
        return 80.0 + i as f64;
    }
    if cp == 0xBC || cp == 0xBD || cp == 0xBE {
        return DIGIT0 + 0.5;
    }
    if (0xA1..=0xBF).contains(&cp) {
        return 40.0 + f64::from(cp - 0xA1) / 100.0;
    }
    if (0x2010..=0x2BFF).contains(&cp) {
        return 90.0 + f64::from(cp - 0x2010) / 10000.0;
    }
    if is_cjk(cp) {
        return CJK + f64::from(cp);
    }
    if (0x30A1..=0x30F6).contains(&lc) {
        return OTHER + f64::from(lc) - f64::from(0x60u32);
    }
    OTHER + f64::from(lc)
}

/// The collation elements of one character: primary, secondary and tertiary weights.
#[derive(Default)]
struct Elements {
    p: Vec<f64>,
    s: Vec<f64>,
    t: Vec<f64>,
}

/// The elements of a character; `None` for the ignorable ones (hyphen, apostrophe, U+2010, U+2011, soft hyphen).
fn char_elements(cp: u32) -> Option<Elements> {
    if cp == 0x2D || cp == 0x27 || cp == 0xAD || cp == 0x2010 || cp == 0x2011 {
        return None;
    }
    if cp < 128
        || index_in(&CURRENCY, cp).is_some()
        || index_in(&MATH, cp).is_some()
        || index_in(&MISC, cp).is_some()
        || cp == 0xBC
        || cp == 0xBD
        || cp == 0xBE
    {
        return Some(Elements { p: vec![base_primary(cp)], s: vec![0.0], t: vec![0.0] });
    }
    if let Some(&(_, base, sec)) = SPECIAL.iter().find(|&&(c, _, _)| c == cp) {
        let mut e = Elements::default();
        for (i, b) in base.bytes().enumerate() {
            e.p.push(base_primary(u32::from(b)));
            e.s.push(if i == 0 { sec } else { 0.0 });
            e.t.push(0.0);
        }
        return Some(e);
    }
    let mut unit_buf = Vec::with_capacity(2);
    crate::utf16::push(&mut unit_buf, cp);
    let decomposed = nfkd(&unit_buf);
    let mut e = Elements::default();
    let compat = (0xFF00..=0xFFEF).contains(&cp);
    for c in code_points(&decomposed) {
        if (0x300..=0x36F).contains(&c) {
            if let Some(last) = e.s.last_mut() {
                let mark = MARK.iter().find(|&&(m, _)| m == c).map_or_else(|| 20.0 + f64::from(c - 0x300), |&(_, w)| w);
                *last = *last * 32.0 + mark;
            }
            continue;
        }
        e.p.push(base_primary(c));
        e.s.push(0.0);
        let third = if (0x30A1..=0x30FA).contains(&c) {
            if compat { 1.0 } else { 2.0 }
        } else if (0x3041..=0x3096).contains(&c) {
            3.0
        } else if compat {
            4.0
        } else {
            0.0
        };
        e.t.push(third);
    }
    if e.p.is_empty() {
        e.p.push(base_primary(cp));
        e.s.push(0.0);
        e.t.push(0.0);
    }
    Some(e)
}

/// The sort key of a text: primary weights, secondary (accents), tertiary (kana width) and the positions of
/// the ignorable characters.
#[derive(Debug, Clone, PartialEq)]
pub struct CollationKey {
    p: Vec<f64>,
    s: Vec<f64>,
    t: Vec<f64>,
    sp: Vec<(usize, u8)>,
}

fn cmp_seq(a: &[f64], b: &[f64]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        if x != y {
            return if x < y { Ordering::Less } else { Ordering::Greater };
        }
    }
    a.len().cmp(&b.len())
}

impl CollationKey {
    /// `keyOf(text)`.
    #[must_use]
    pub fn new(text: &[u16]) -> Self {
        let mut key = CollationKey { p: Vec::new(), s: Vec::new(), t: Vec::new(), sp: Vec::new() };
        let mut pos = 0usize;
        for cp in code_points(text) {
            if cp == 0xAD {
                continue;
            }
            match char_elements(cp) {
                None => key.sp.push((pos, if cp == 0x27 { 1 } else { 2 })),
                Some(e) => {
                    key.p.extend(e.p);
                    key.s.extend(e.s);
                    key.t.extend(e.t);
                }
            }
            pos += 1;
        }
        key
    }

    /// `compareText` on two keys.
    #[must_use]
    pub fn compare(&self, other: &Self) -> Ordering {
        let c1 = cmp_seq(&self.p, &other.p);
        if c1 != Ordering::Equal {
            return c1;
        }
        let c2 = cmp_seq(&self.s, &other.s);
        if c2 != Ordering::Equal {
            return c2;
        }
        let c3 = cmp_seq(&self.t, &other.t);
        if c3 != Ordering::Equal {
            return c3;
        }
        let (x, y) = (&self.sp, &other.sp);
        if x.is_empty() || y.is_empty() {
            return x.len().cmp(&y.len());
        }
        for (a, b) in x.iter().zip(y) {
            if a.0 != b.0 {
                return if a.0 > b.0 { Ordering::Less } else { Ordering::Greater };
            }
            if a.1 != b.1 {
                return if a.1 < b.1 { Ordering::Less } else { Ordering::Greater };
            }
        }
        x.len().cmp(&y.len())
    }

    /// `collationKey(text)`: a string that is equal for two texts exactly when they compare equal.
    #[must_use]
    pub fn to_key_string(&self) -> String {
        let join = |v: &[f64]| v.iter().map(|&n| uo_jsnum::format::to_js_string(n)).collect::<Vec<_>>().join(",");
        let sp = self.sp.iter().map(|&(pos, w)| format!("{pos}:{w}")).collect::<Vec<_>>().join(",");
        format!("{}|{}|{}|{}", join(&self.p), join(&self.s), join(&self.t), sp)
    }
}

/// `compareText(a, b)`: the Excel order of two texts.
#[must_use]
pub fn compare_text(a: &[u16], b: &[u16]) -> Ordering {
    if a == b {
        return Ordering::Equal;
    }
    CollationKey::new(a).compare(&CollationKey::new(b))
}

/// `textCollateEqual(a, b)`: the `=` operator and UNIQUE (`ß` = `ss`, `a` is not `á`).
#[must_use]
pub fn text_collate_equal(a: &[u16], b: &[u16]) -> bool {
    compare_text(a, b) == Ordering::Equal
}

/// `collationKey(text)`.
#[must_use]
pub fn collation_key(text: &[u16]) -> String {
    CollationKey::new(text).to_key_string()
}
