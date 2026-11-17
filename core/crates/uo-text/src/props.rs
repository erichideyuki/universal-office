//! The Unicode property classes the Write engine tests with `RegExp` and the `u` flag: `\p{L}`,
//! `\p{M}`, `\p{N}` and `\p{P}`, as V8 13.6 (ICU, Unicode 17.0) matches them. A code point that is
//! not in any class (a lone surrogate, unassigned, controls, symbols) matches none.

use crate::tables as t;
use crate::utf16::decode_at;

/// `\p{L}` (Letter).
pub const LETTER: u8 = 1;
/// `\p{M}` (Mark).
pub const MARK: u8 = 2;
/// `\p{N}` (Number).
pub const NUMBER: u8 = 4;
/// `\p{P}` (Punctuation).
pub const PUNCTUATION: u8 = 8;

/// The classes of `cp` as a mask of [`LETTER`], [`MARK`], [`NUMBER`], [`PUNCTUATION`] (they never overlap).
#[must_use]
pub fn mask(cp: u32) -> u8 {
    let idx = t::PROPS.partition_point(|&(first, _)| first <= cp);
    match idx.checked_sub(1).and_then(|i| t::PROPS.get(i)) {
        Some(&(_, tail)) if cp <= tail & 0x00FF_FFFF => u8::try_from(tail >> 24).unwrap_or(0),
        _ => 0,
    }
}

/// `/\p{L}/u.test(String.fromCodePoint(cp))`.
#[must_use]
pub fn is_letter(cp: u32) -> bool {
    mask(cp) & LETTER != 0
}

/// `\p{M}`.
#[must_use]
pub fn is_mark(cp: u32) -> bool {
    mask(cp) & MARK != 0
}

/// `\p{N}`.
#[must_use]
pub fn is_number(cp: u32) -> bool {
    mask(cp) & NUMBER != 0
}

/// `\p{P}`.
#[must_use]
pub fn is_punctuation(cp: u32) -> bool {
    mask(cp) & PUNCTUATION != 0
}

/// `\p{L}` or `\p{M}`: the `[\p{L}\p{M}]` of the hyphenation and text-layout letter tests.
#[must_use]
pub fn is_letter_or_mark(cp: u32) -> bool {
    mask(cp) & (LETTER | MARK) != 0
}

/// `\p{Ll}` (Lowercase_Letter): the small-caps split of the Slides text layout (`text-layout.ts:171`).
#[must_use]
pub fn is_lowercase_letter(cp: u32) -> bool {
    let idx = t::LOWER_LETTER.partition_point(|&(first, _)| first <= cp);
    matches!(idx.checked_sub(1).and_then(|i| t::LOWER_LETTER.get(i)), Some(&(_, last)) if cp <= last)
}

/// `/^[...]$/u.test(s)` for a class given as a mask: `s` is exactly one code point and it is in the class.
#[must_use]
pub fn is_single_code_point_in(units: &[u16], classes: u8) -> bool {
    matches!(decode_at(units, 0), Some((cp, width)) if width == units.len() && mask(cp) & classes != 0)
}
