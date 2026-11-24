//! Excel's text order and number comparison (`collate.ts` + `collation.ts`). The collation itself is the one port
//! in `uo_text::collation` (shared with Write's table sort); this module adds the JS-string entry points the calc
//! uses and an allocation-free path for ASCII text (the common case of comparisons, SORT, MATCH, criteria): for
//! ASCII every character is one primary weight with zero secondary/tertiary weights, or an ignorable (`-`, `'`),
//! so the order is the primary sequence, then the ignorables' positions — exactly what the general keys give
//! (checked against `uo_text` exhaustively on random ASCII pairs and by the parity captures). Latin-1 text (the
//! accented Portuguese/Spanish case) compares through a table of the 256 characters' collation elements, built once
//! from `uo_text`'s own keys, streamed level by level without allocating.

use std::cmp::Ordering;
use std::sync::OnceLock;

use uo_text::collation::CollationKey;

use crate::jstext::with_u16;
use crate::strings::JsStr;

/// Primary weights of the ASCII characters (`basePrimary`); 0 = ignorable (`-`, `'`).
const ASCII_WEIGHT: [u32; 128] = {
    const PUNCT: &[u8] = b"!\"#$%&()*,./:;?@[\\]^_`{|}~";
    const MATH: &[u8] = b"+<=>";
    let mut t = [0u32; 128];
    let mut c = 0;
    while c < 128 {
        t[c] = if c < 32 || c == 0x7f {
            1
        } else if c == b' ' as usize {
            10
        } else if c >= b'0' as usize && c <= b'9' as usize {
            1000 + (c - b'0' as usize) as u32
        } else if c >= b'a' as usize && c <= b'z' as usize {
            2000 + (c - b'a' as usize) as u32
        } else if c >= b'A' as usize && c <= b'Z' as usize {
            2000 + (c - b'A' as usize) as u32
        } else if c == b'-' as usize || c == b'\'' as usize {
            0
        } else {
            let mut w = 0;
            let mut i = 0;
            while i < PUNCT.len() {
                if PUNCT[i] as usize == c {
                    w = 11 + i as u32;
                }
                i += 1;
            }
            let mut i = 0;
            while i < MATH.len() {
                if MATH[i] as usize == c {
                    w = 70 + i as u32;
                }
                i += 1;
            }
            w
        };
        c += 1;
    }
    t
};

fn to_i32(o: Ordering) -> i32 {
    match o {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// `compareText` of two ASCII texts without allocating.
fn compare_ascii(a: &[u8], b: &[u8]) -> Ordering {
    let w = |c: &u8| ASCII_WEIGHT[usize::from(*c & 0x7f)];
    let pa = a.iter().map(w).filter(|&x| x != 0);
    let pb = b.iter().map(w).filter(|&x| x != 0);
    let c1 = pa.cmp(pb);
    if c1 != Ordering::Equal {
        return c1;
    }
    // equal primaries ⇒ equal lengths ⇒ equal (zero) secondary and tertiary sequences; then the ignorables
    let ign = |c: u8| match c {
        b'\'' => Some(1u8),
        b'-' => Some(2u8),
        _ => None,
    };
    let xa = a.iter().enumerate().filter_map(|(i, &c)| ign(c).map(|w| (i, w)));
    let xb = b.iter().enumerate().filter_map(|(i, &c)| ign(c).map(|w| (i, w)));
    compare_ignorables(xa, xb)
}

/// The collation elements of one Latin-1 character (at most two: `ß` = `ss`, `æ` = `ae`).
#[derive(Clone, Copy, Default)]
struct L1 {
    /// Number of elements (0 = ignorable or skipped).
    n: u8,
    /// (primary, secondary, tertiary) per element.
    w: [[f64; 3]; 2],
    /// Ignorable weight (`'` 1, `-` 2), 0 otherwise.
    ign: u8,
    /// The soft hyphen: removed before the positions are counted.
    skip: bool,
}

fn latin1_table() -> &'static [L1; 256] {
    static T: OnceLock<[L1; 256]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [L1::default(); 256];
        for (cp, e) in t.iter_mut().enumerate() {
            let key = CollationKey::new(&[cp as u16]);
            let w = key.weights();
            e.skip = w.primary.is_empty() && w.ignorables.is_empty();
            e.ign = w.ignorables.first().map_or(0, |x| x.1);
            e.n = w.primary.len().min(2) as u8;
            for k in 0..usize::from(e.n) {
                e.w[k] = [w.primary[k], w.secondary[k], w.tertiary[k]];
            }
        }
        t
    })
}

fn level_seq<'a>(t: &'a [L1; 256], s: &'a [u8], level: usize) -> impl Iterator<Item = f64> + 'a {
    s.iter().flat_map(move |&c| {
        let e = t[usize::from(c)];
        (0..usize::from(e.n)).map(move |k| e.w[k][level])
    })
}

/// The ignorables of a Latin-1 text: positions count the characters (the soft hyphen excluded).
fn ign_seq<'a>(t: &'a [L1; 256], s: &'a [u8]) -> impl Iterator<Item = (usize, u8)> + 'a {
    let mut pos = 0usize;
    s.iter().filter_map(move |&c| {
        let e = t[usize::from(c)];
        if e.skip {
            return None;
        }
        let here = pos;
        pos += 1;
        (e.ign != 0).then_some((here, e.ign))
    })
}

/// `compareText` of two Latin-1 texts without allocating (levels streamed from the table).
fn compare_latin1(a: &[u8], b: &[u8]) -> Ordering {
    let t = latin1_table();
    for level in 0..3 {
        let o = level_seq(t, a, level).partial_cmp(level_seq(t, b, level)).unwrap_or(Ordering::Equal);
        if o != Ordering::Equal {
            return o;
        }
    }
    compare_ignorables(ign_seq(t, a), ign_seq(t, b))
}

/// The ignorables rule of `compareText`: none sorts first; then the one with them further right; then the weights.
fn compare_ignorables(mut xa: impl Iterator<Item = (usize, u8)>, mut xb: impl Iterator<Item = (usize, u8)>) -> Ordering {
    let (first_a, first_b) = (xa.next(), xb.next());
    let (Some(mut x), Some(mut y)) = (first_a, first_b) else {
        return match (first_a.is_some(), first_b.is_some()) {
            (false, false) => Ordering::Equal,
            (true, _) => Ordering::Greater,
            (false, true) => Ordering::Less,
        };
    };
    loop {
        if x.0 != y.0 {
            return if x.0 > y.0 { Ordering::Less } else { Ordering::Greater };
        }
        if x.1 != y.1 {
            return if x.1 < y.1 { Ordering::Less } else { Ordering::Greater };
        }
        match (xa.next(), xb.next()) {
            (Some(p), Some(q)) => {
                x = p;
                y = q;
            }
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
        }
    }
}

/// `compareText(a, b)` on JS strings: -1, 0 or 1.
#[must_use]
pub fn compare_text_js(a: JsStr<'_>, b: JsStr<'_>) -> i32 {
    to_i32(compare_text_ord(a, b))
}

/// `compareText(a, b)` as an [`Ordering`].
#[must_use]
pub fn compare_text_ord(a: JsStr<'_>, b: JsStr<'_>) -> Ordering {
    if a.eq_units(b) {
        return Ordering::Equal;
    }
    if let (JsStr::Latin1(x), JsStr::Latin1(y)) = (a, b) {
        return if x.is_ascii() && y.is_ascii() { compare_ascii(x, y) } else { compare_latin1(x, y) };
    }
    with_u16(a, |au| with_u16(b, |bu| uo_text::collation::compare_text(au, bu)))
}

/// `textEquals` / `textCollateEqual`: the `=` operator and UNIQUE (`ß` = `ss`, `a` ≠ `á`).
#[must_use]
pub fn text_equals(a: JsStr<'_>, b: JsStr<'_>) -> bool {
    compare_text_ord(a, b) == Ordering::Equal
}

/// A text prepared for many comparisons (criteria, sorts): its collation key is built once unless it is ASCII.
#[derive(Clone, Debug)]
pub struct PreparedText {
    units: Vec<u16>,
    /// The text as Latin-1 bytes when every unit fits (compared through the fast paths).
    narrow: Option<Box<[u8]>>,
    key: CollationKey,
}

impl PreparedText {
    /// Prepares `s`.
    #[must_use]
    pub fn new(s: JsStr<'_>) -> Self {
        let units = s.to_utf16();
        let narrow = units.iter().all(|&u| u <= 0xFF).then(|| units.iter().map(|&u| u as u8).collect());
        let key = CollationKey::new(&units);
        Self { units, narrow, key }
    }

    /// The text's code units.
    #[must_use]
    pub fn units(&self) -> &[u16] {
        &self.units
    }

    /// `compareText(v, self)`.
    #[must_use]
    pub fn compare_from(&self, v: JsStr<'_>) -> Ordering {
        if let (Some(t), JsStr::Latin1(x)) = (&self.narrow, v) {
            return if **t == *x {
                Ordering::Equal
            } else if t.is_ascii() && x.is_ascii() {
                compare_ascii(x, t)
            } else {
                compare_latin1(x, t)
            };
        }
        if crate::jstext::eq(&self.units, v) {
            return Ordering::Equal;
        }
        with_u16(v, |vu| CollationKey::new(vu).compare(&self.key))
    }
}

/// `collationKey(text)`: equal keys ⇔ `compareText` = 0 (UNIQUE/GROUPBY hashing).
#[must_use]
pub fn collation_key(text: JsStr<'_>) -> String {
    with_u16(text, uo_text::collation::collation_key)
}

/// `Number(x.toPrecision(15))`.
fn precision15(x: f64) -> f64 {
    match uo_jsnum::format::to_precision(x, 15) {
        Ok(s) => uo_jsnum::parse::to_number(&s),
        Err(_) => x,
    }
}

/// `round15(x)`: x rounded to 15 significant digits (how Excel compares).
#[must_use]
pub fn round15(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    // the exact integer path (no text, no allocation); the text path outside its range
    crate::math_util::round15_exact(x).unwrap_or_else(|| precision15(x))
}

/// `compareNumbers(a, b)`: Excel's operators: equal when they agree to 15 significant digits
/// (`=0,1+0,2=0,3` → TRUE). Returns -1, 0 or 1.
#[must_use]
pub fn compare_numbers(a: f64, b: f64) -> i32 {
    if a == b {
        return 0;
    }
    let m = uo_jsnum::math::max(a.abs(), b.abs());
    if (a - b).abs() > m * 1e-13 {
        return if a < b { -1 } else { 1 };
    }
    let (x, y) = (round15(a), round15(b));
    if x == y {
        0
    } else if x < y {
        -1
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_fast_path_equals_keys() {
        let mut s: u32 = 0x1234_5678;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            s
        };
        const ALPHA: &[u8] = b"aAbB09 -'!+<=>~_.,zZ\x01\x7f";
        for _ in 0..40_000 {
            let mut mk = || {
                let n = (next() % 6) as usize;
                (0..n).map(|_| ALPHA[(next() as usize) % ALPHA.len()]).collect::<Vec<u8>>()
            };
            let (a, b) = (mk(), mk());
            let au: Vec<u16> = a.iter().map(|&c| u16::from(c)).collect();
            let bu: Vec<u16> = b.iter().map(|&c| u16::from(c)).collect();
            let want = uo_text::collation::compare_text(&au, &bu);
            assert_eq!(compare_text_ord(JsStr::Latin1(&a), JsStr::Latin1(&b)), want, "{a:?} {b:?}");
            let p = PreparedText::new(JsStr::Latin1(&b));
            assert_eq!(p.compare_from(JsStr::Latin1(&a)), want);
        }
    }

    #[test]
    fn latin1_table_equals_keys() {
        let mut s: u32 = 0x9e37_79b9;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            s
        };
        const BIAS: &[u8] = b"aeAE-'\xad\xdf";
        for _ in 0..60_000 {
            let mut mk = || {
                let n = (next() % 6) as usize;
                (0..n)
                    .map(|_| {
                        let r = next();
                        if r % 3 == 0 { BIAS[(r as usize / 3) % BIAS.len()] } else { (r >> 8) as u8 }
                    })
                    .collect::<Vec<u8>>()
            };
            let (a, b) = (mk(), mk());
            let au: Vec<u16> = a.iter().map(|&c| u16::from(c)).collect();
            let bu: Vec<u16> = b.iter().map(|&c| u16::from(c)).collect();
            let want = uo_text::collation::compare_text(&au, &bu);
            assert_eq!(compare_text_ord(JsStr::Latin1(&a), JsStr::Latin1(&b)), want, "{a:?} {b:?}");
            assert_eq!(PreparedText::new(JsStr::Latin1(&b)).compare_from(JsStr::Latin1(&a)), want, "{a:?} {b:?}");
        }
    }

    #[test]
    fn numbers() {
        assert_eq!(compare_numbers(0.1 + 0.2, 0.3), 0);
        assert_eq!(compare_numbers(1e15 + 5.0, 1e15), 1);
        assert_eq!(compare_numbers(1e15 + 4.0, 1e15), 0);
    }
}
