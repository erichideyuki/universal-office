//! UTF-16 as JavaScript sees it: a string is a sequence of `u16` code units, and a surrogate that is
//! not half of a valid pair is a lone surrogate that stays what it is (`String.fromCodePoint(0xD800)`,
//! `for (const ch of s)` and `s.codePointAt(i)` all yield it as a "code point" of its own).
//!
//! A Rust `&str` cannot hold a lone surrogate, so every function of this crate takes `&[u16]`
//! (the `_str` variants are for text that is known to be valid).

const HIGH: std::ops::RangeInclusive<u16> = 0xD800..=0xDBFF;
const LOW: std::ops::RangeInclusive<u16> = 0xDC00..=0xDFFF;

/// The code point at `units[i]` and its width in code units (1 or 2): a high surrogate followed by
/// a low one is one supplementary code point, anything else is the unit itself (lone surrogates
/// included). `None` past the end.
#[must_use]
#[inline]
pub fn decode_at(units: &[u16], i: usize) -> Option<(u32, usize)> {
    let &u = units.get(i)?;
    if HIGH.contains(&u)
        && let Some(&next) = units.get(i + 1)
        && LOW.contains(&next)
    {
        return Some((0x1_0000 + ((u32::from(u) - 0xD800) << 10) + (u32::from(next) - 0xDC00), 2));
    }
    Some((u32::from(u), 1))
}

/// The code point that ends just before `units[end]` (a pair counts as one) and its width; `None`
/// when `end` is 0.
#[must_use]
#[inline]
pub fn decode_before(units: &[u16], end: usize) -> Option<(u32, usize)> {
    let &last = units.get(end.checked_sub(1)?)?;
    if LOW.contains(&last)
        && end >= 2
        && let Some(&first) = units.get(end - 2)
        && HIGH.contains(&first)
    {
        return Some((0x1_0000 + ((u32::from(first) - 0xD800) << 10) + (u32::from(last) - 0xDC00), 2));
    }
    Some((u32::from(last), 1))
}

/// The code points of a UTF-16 string as `Array.from(s)` / `for (const ch of s)` yield them.
#[derive(Debug, Clone)]
pub struct CodePoints<'a> {
    units: &'a [u16],
    pos: usize,
}

/// `for (const ch of s)` over code units, as code points (lone surrogates as themselves).
#[must_use]
pub fn code_points(units: &[u16]) -> CodePoints<'_> {
    CodePoints { units, pos: 0 }
}

impl Iterator for CodePoints<'_> {
    type Item = u32;

    #[inline]
    fn next(&mut self) -> Option<u32> {
        let (cp, width) = decode_at(self.units, self.pos)?;
        self.pos += width;
        Some(cp)
    }
}

/// Appends a code point as UTF-16: one unit below U+10000 (a surrogate value stays a lone surrogate),
/// a pair from U+10000 up. Values above U+10FFFF (never produced by the tables) are dropped.
#[inline]
pub fn push(out: &mut Vec<u16>, cp: u32) {
    if let Ok(unit) = u16::try_from(cp) {
        out.push(unit);
    } else if cp <= 0x10_FFFF {
        let v = cp - 0x1_0000;
        out.push(0xD800 + u16::try_from(v >> 10).unwrap_or(0));
        out.push(0xDC00 + u16::try_from(v & 0x3FF).unwrap_or(0));
    }
}

/// `s.length` units of a Rust string.
#[must_use]
pub fn from_str(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// A Rust string from UTF-16, each lone surrogate as U+FFFD (what `TextEncoder` writes for it).
#[must_use]
pub fn to_string_lossy(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}
