//! The `String.prototype` primitives the text family uses, over UTF-16 code units with the JS argument rules
//! (`ToIntegerOrInfinity`, negative positions relative to the end, `NaN` as 0), and a small output buffer that stays
//! on the stack for short results (a substring or a short text costs no heap allocation before it reaches the pass
//! arena).

use uo_jsnum::math::{max as jmax, min as jmin};
use uo_text::utf16::decode_at;

/// `ToIntegerOrInfinity(x)` (NaN → 0).
#[inline]
pub(super) fn to_int(x: f64) -> f64 {
    if x.is_nan() { 0.0 } else { uo_jsnum::math::trunc(x) + 0.0 }
}

/// A position clamped into `0..=len` (`s.slice` bound: negative counts from the end).
#[inline]
pub(super) fn rel(x: f64, len: usize) -> usize {
    let l = len as f64;
    let i = to_int(x);
    let r = if i < 0.0 { jmax(l + i, 0.0) } else { jmin(i, l) };
    crate::cast::trunc_usize(r)
}

/// A position clamped into `0..=len` without the negative rule (`indexOf`'s `position`, `substring`).
#[inline]
pub(super) fn clamp(x: f64, len: usize) -> usize {
    let i = to_int(x);
    crate::cast::trunc_usize(jmin(jmax(i, 0.0), len as f64))
}

/// `s.slice(start, end)`.
#[inline]
pub(super) fn slice(s: &[u16], start: f64, end: f64) -> &[u16] {
    let a = rel(start, s.len());
    let b = rel(end, s.len());
    if a < b { &s[a..b] } else { &[] }
}

/// `s.slice(start)`.
#[inline]
pub(super) fn slice_from(s: &[u16], start: f64) -> &[u16] {
    &s[rel(start, s.len())..]
}

/// `hay.indexOf(p, from)` for a clamped `from`.
pub(super) fn index_of(hay: &[u16], p: &[u16], from: usize) -> Option<usize> {
    let from = from.min(hay.len());
    if p.is_empty() {
        return Some(from);
    }
    if p.len() > hay.len() {
        return None;
    }
    let first = p[0];
    let last = hay.len() - p.len();
    let mut i = from;
    while i <= last {
        i += hay[i..=last].iter().position(|&u| u == first)?;
        if hay[i..i + p.len()] == *p {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// `hay.lastIndexOf(p, position)` (`position` NaN = +∞).
pub(super) fn last_index_of(hay: &[u16], p: &[u16], position: f64) -> Option<usize> {
    let pos = if position.is_nan() { f64::INFINITY } else { to_int(position) };
    let start = crate::cast::trunc_usize(jmin(jmax(pos, 0.0), hay.len() as f64));
    if p.len() > hay.len() {
        return None;
    }
    let mut i = start.min(hay.len() - p.len());
    loop {
        if hay[i..i + p.len()] == *p {
            return Some(i);
        }
        if i == 0 {
            return None;
        }
        i -= 1;
    }
}

/// `true` when the text holds a surrogate unit (`/[\uD800-\uDFFF]/`).
#[inline]
pub(super) fn has_surr(s: &[u16]) -> bool {
    s.iter().any(|&u| (0xD800..=0xDFFF).contains(&u))
}

/// The code points of a text with their unit offsets (`Array.from(s)`: a lone surrogate is one element).
pub(super) fn chars(s: &[u16]) -> impl Iterator<Item = (u32, usize, usize)> + '_ {
    let mut pos = 0;
    core::iter::from_fn(move || {
        let (cp, w) = decode_at(s, pos)?;
        let at = pos;
        pos += w;
        Some((cp, at, w))
    })
}

/// `[...s].length`.
pub(super) fn cp_count(s: &[u16]) -> usize {
    if has_surr(s) { chars(s).count() } else { s.len() }
}

/// `String.fromCharCode(x)` (`ToUint16`).
#[inline]
pub(super) fn from_char_code(x: f64) -> u16 {
    uo_jsnum::conv::to_uint16(x)
}

/// Appends a code point (below 0x110000; a lone surrogate as itself) to a unit vector.
pub(super) fn push_cp(out: &mut Vec<u16>, cp: u64) {
    match u16::try_from(cp) {
        Ok(c) => out.push(c),
        Err(_) => {
            let c = cp.saturating_sub(0x1_0000);
            out.extend([0xD800 + ((c >> 10) & 0x3FF) as u16, 0xDC00 + (c & 0x3FF) as u16]);
        }
    }
}

/// Units of a Rust string.
pub(super) fn u(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// `a === b` for a unit slice and a Rust (ASCII or not) string.
pub(super) fn eq_str(a: &[u16], b: &str) -> bool {
    a.iter().copied().eq(b.encode_utf16())
}

/// An ASCII-only case-insensitive comparison of a unit against an ASCII byte (the `/i` flag over ASCII pattern
/// text without `u`: a non-ASCII unit never canonicalizes to ASCII).
#[inline]
pub(super) fn ascii_ci(a: u16, b: u8) -> bool {
    a < 0x80 && (a as u8).eq_ignore_ascii_case(&b)
}

/// `s.slice(i, i + p.len())` equals the ASCII text `p` ignoring ASCII case.
pub(super) fn starts_ci(s: &[u16], i: usize, p: &[u8]) -> bool {
    s.get(i..i + p.len()).is_some_and(|w| w.iter().zip(p).all(|(&a, &b)| ascii_ci(a, b)))
}

/// Inline capacity of [`Buf`] in units.
const INLINE: usize = 160;

/// An output text: on the stack up to [`INLINE`] units, then on the heap.
pub(super) struct Buf {
    len: usize,
    inline: [u16; INLINE],
    heap: Vec<u16>,
    on_heap: bool,
}

impl Buf {
    /// An empty buffer.
    pub(super) fn new() -> Self {
        Buf { len: 0, inline: [0; INLINE], heap: Vec::new(), on_heap: false }
    }

    fn spill(&mut self, extra: usize) {
        if !self.on_heap && self.len + extra > INLINE {
            self.heap.reserve(self.len + extra);
            self.heap.extend_from_slice(&self.inline[..self.len]);
            self.on_heap = true;
        }
    }

    /// Appends units.
    pub(super) fn extend(&mut self, s: &[u16]) {
        self.spill(s.len());
        if !self.on_heap {
            self.inline[self.len..self.len + s.len()].copy_from_slice(s);
        } else {
            self.heap.extend_from_slice(s);
        }
        self.len += s.len();
    }

    /// Appends one unit.
    pub(super) fn push(&mut self, c: u16) {
        self.extend(&[c]);
    }

    /// Appends ASCII text.
    pub(super) fn ascii(&mut self, s: &str) {
        for b in s.bytes() {
            self.push(u16::from(b));
        }
    }

    /// Appends a code point (a lone surrogate as itself).
    pub(super) fn cp(&mut self, cp: u32) {
        if let Ok(c) = u16::try_from(cp) {
            self.push(c);
        } else {
            let c = cp - 0x1_0000;
            self.extend(&[0xD800 + (c >> 10) as u16, 0xDC00 + (c & 0x3FF) as u16]);
        }
    }

    /// The units.
    pub(super) fn as_slice(&self) -> &[u16] {
        if self.on_heap { &self.heap } else { &self.inline[..self.len] }
    }

    /// Length in units.
    pub(super) fn len(&self) -> usize {
        self.len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_follow_js() {
        let s = u("abcdef");
        assert_eq!(slice(&s, 1.0, 3.0), &u("bc")[..]);
        assert_eq!(slice(&s, -2.0, f64::INFINITY), &u("ef")[..]);
        assert_eq!(slice(&s, f64::NAN, f64::NAN), &[] as &[u16]);
        assert_eq!(slice(&s, 0.0, -1.0), &u("abcde")[..]);
        assert_eq!(index_of(&s, &u("cd"), 0), Some(2));
        assert_eq!(index_of(&s, &u(""), 9), Some(6));
        assert_eq!(last_index_of(&s, &u("b"), f64::NAN), Some(1));
        assert_eq!(last_index_of(&s, &u("f"), 2.0), None);
        let mut b = Buf::new();
        for _ in 0..100 {
            b.ascii("xy");
        }
        assert_eq!(b.len(), 200);
        assert!(b.as_slice().iter().all(|&c| c == u16::from(b'x') || c == u16::from(b'y')));
    }
}
