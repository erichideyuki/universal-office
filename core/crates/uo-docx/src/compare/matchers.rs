//! The regex sites of the diff (G9): hand matchers over UTF-16 code units, linear, the exact behaviour
//! of the TS `RegExp` on any text (checked against V8 by `js/regex-sites/d5a-compare.mjs` and
//! `tests/regex_d5a_compare.rs`).
//!
//! | site | TS | matcher |
//! |---|---|---|
//! | `word_tokens` | `diff.ts:204` `/\S+\|\s+/g` | [`word_tokens`] |
//! | `has_word` | `diff.ts:243` `/\S/.test(text)` | [`has_non_space`] |
//!
//! `\s` is the JS white space set ([`uo_jsnum::text::is_js_space_unit`]); a lone surrogate is `\S`.

use uo_jsnum::text::is_js_space_unit;

/// The matches of `/\S+|\s+/g` over `text`: `(start, end)` in code units, runs of non-space and runs
/// of space alternating.
#[must_use]
pub fn word_tokens(text: &[u16]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let space = text.get(i).is_some_and(|&u| is_js_space_unit(u));
        let mut j = i + 1;
        while text.get(j).is_some_and(|&u| is_js_space_unit(u) == space) {
            j += 1;
        }
        out.push((i, j));
        i = j;
    }
    out
}

/// `/\S/.test(text)`.
#[must_use]
pub fn has_non_space(text: &[u16]) -> bool {
    text.iter().any(|&u| !is_js_space_unit(u))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alternates_words_and_spaces() {
        let u: Vec<u16> = " ab  c\u{a0}d".encode_utf16().collect();
        assert_eq!(word_tokens(&u), [(0, 1), (1, 3), (3, 5), (5, 6), (6, 7), (7, 8)]);
        assert!(word_tokens(&[]).is_empty());
        assert!(has_non_space(&u));
        assert!(!has_non_space(&[0x20, 0xa0]));
    }
}
