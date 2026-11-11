//! `editor/src/text-layout.ts` 178–222: the Manual Hyphenation helpers (`wordAround`,
//! `manualPoints`, `offsetAfterLetters`).

use crate::text::Utf16;

/// `ManualHyphenation`: the word the dialog proposes.
#[derive(Debug, Clone, PartialEq)]
pub struct ManualHyphenation {
    /// the word, without the optional hyphens it already has
    pub word: Utf16,
    /// the language (`w:lang`)
    pub lang: String,
    /// where there may be a hyphen: the number of letters before it, in order
    pub points: Vec<u32>,
    /// the proposed point (the automatic hyphenation's)
    pub suggested: u32,
    /// the last point before the margin
    pub limit: u32,
}

/// `LETTER.test(c)` for ONE UTF-16 code unit (`/^[\p{L}\p{M}]$/u`: a lone surrogate matches
/// nothing).
pub(super) fn is_letter_unit(u: u16) -> bool {
    char::from_u32(u32::from(u)).is_some_and(|c| uo_text::props::is_letter_or_mark(c as u32))
}

/// `wordAround(text, at)`: the word (letters and optional hyphens) around `at`: `[start, end)`.
#[must_use]
pub fn word_around(text: &[u16], at: usize) -> (usize, usize) {
    let is_w = |u: u16| is_letter_unit(u) || u == 0x00AD;
    let mut a = at.min(text.len());
    let mut b = a;
    while a > 0 && text.get(a - 1).is_some_and(|&u| is_w(u)) {
        a -= 1;
    }
    while text.get(b).is_some_and(|&u| is_w(u)) {
        b += 1;
    }
    (a, b)
}

/// `manualPoints(raw, patternPoints)`: the points of a word: the patterns' and the optional
/// hyphens it already has (the number of letters before them), without repeats, in order. The word
/// is `raw` without U+00AD.
#[must_use]
pub fn manual_points(raw: &[u16], pattern_points: &[u32]) -> (Vec<u16>, Vec<u32>) {
    let word: Vec<u16> = raw.iter().copied().filter(|&u| u != 0x00AD).collect();
    let mut shy: Vec<u32> = Vec::new();
    let mut n = 0u32;
    for &u in raw {
        if u == 0x00AD {
            shy.push(n);
        } else {
            n += 1;
        }
    }
    let len = word.len();
    let mut points: Vec<u32> = Vec::new();
    for &p in pattern_points.iter().chain(shy.iter()) {
        if p > 0 && (p as usize) < len && !points.contains(&p) {
            points.push(p);
        }
    }
    points.sort_unstable();
    (word, points)
}

/// `offsetAfterLetters(raw, start, letters)`: the offset in the text (with the optional hyphens)
/// after `letters` letters of the word.
#[must_use]
pub fn offset_after_letters(raw: &[u16], start: usize, letters: f64) -> usize {
    let mut n = 0.0_f64;
    for (i, &u) in raw.iter().enumerate() {
        if u == 0x00AD {
            continue;
        }
        if n == letters {
            return start + i;
        }
        n += 1.0;
    }
    start + raw.len()
}
