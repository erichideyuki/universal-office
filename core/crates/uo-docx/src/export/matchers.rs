//! The regex sites of the exporters (G9): hand matchers, linear, the exact behaviour of the TS
//! `RegExp` on any text (checked against V8 by `js/regex-sites/d5a-export.mjs` and
//! `tests/regex_d5a_export.rs`).
//!
//! | site | TS | matcher |
//! |---|---|---|
//! | `heading_level_1` | `model.ts:78` `/(?:heading\|t[íi]tulo\|titulo)\s*([1-9])/i` | [`heading_level_of`] |
//! | `heading_level_2` | `model.ts:80` `/^h([1-6])$/i` | [`heading_level_of`] |
//! | `heading_level_3` | `model.ts:82` `/^(title\|t[íi]tulo)$/i` | [`heading_level_of`] |
//! | `escape_md` | `markdown.ts:31` `/([\\`*_{}\[\]<>\|])/g` → `\\$1` | [`escape_md_into`] |
//! | `lead_space`, `trail_space` | `markdown.ts:35–36` `/^\s*/`, `/\s*$/` | [`lead_space_len`], [`trail_space_start`] |
//! | `cell_pipe`, `cell_lf` | `markdown.ts:86` `/\|/g` → `\\|`, `/\n/g` → ` ` | [`cell_text_escape`] |
//! | `collapse_blank` | `markdown.ts:143` `/\n{3,}/g` → `\n\n` | [`collapse_blank_lines`] |
//!
//! `/i` without `u` canonicalizes with the simple upper-case mapping of one code unit and never
//! maps a non-ASCII character onto an ASCII one, so the letters of these patterns match their ASCII
//! case variants, and `í` matches `Í`; nothing else (U+0131 and U+212A stay out).

use uo_jsnum::text::is_js_space;

/// `a` equals `want` (lower case, ASCII or `í`) under `/i`.
fn ci_eq(a: char, want: char) -> bool {
    if want == 'í' {
        return a == 'í' || a == 'Í';
    }
    a == want || a == want.to_ascii_uppercase()
}

/// The number of characters of `pat` matched case-insensitively at the start of `cs`.
fn match_ci(cs: &[char], pat: &str) -> Option<usize> {
    let mut n = 0;
    for (i, w) in pat.chars().enumerate() {
        if !ci_eq(*cs.get(i)?, w) {
            return None;
        }
        n = i + 1;
    }
    Some(n)
}

/// `headingLevel` over a style id: 1..6, or 0 when it is not a heading.
#[must_use]
pub fn heading_level_of(id: &str) -> u32 {
    let cs: Vec<char> = id.chars().collect();
    for s in 0..cs.len() {
        let tail = cs.get(s..).unwrap_or(&[]);
        for kw in ["heading", "título", "titulo"] {
            let Some(n) = match_ci(tail, kw) else { continue };
            let mut j = s + n;
            while cs.get(j).is_some_and(|&c| is_js_space(c)) {
                j += 1;
            }
            if let Some(d) = cs.get(j).and_then(|c| c.to_digit(10)).filter(|d| (1..=9).contains(d)) {
                return d.min(6);
            }
        }
    }
    if cs.len() == 2
        && cs.first().is_some_and(|&c| ci_eq(c, 'h'))
        && let Some(d) = cs.get(1).and_then(|c| c.to_digit(10)).filter(|d| (1..=6).contains(d))
    {
        return d;
    }
    if match_ci(&cs, "title") == Some(cs.len()) || match_ci(&cs, "título") == Some(cs.len()) || match_ci(&cs, "titulo") == Some(cs.len()) {
        return 1;
    }
    0
}

/// `s.replace(/([\\`*_{}\[\]<>|])/g, '\\$1')` over code units.
pub fn escape_md_into(out: &mut Vec<u16>, s: &[u16]) {
    for &u in s {
        if matches!(u, 0x5c | 0x60 | 0x2a | 0x5f | 0x7b | 0x7d | 0x5b | 0x5d | 0x3c | 0x3e | 0x7c) {
            out.push(0x5c);
        }
        out.push(u);
    }
}

/// The length of `/^\s*/` over the units.
#[must_use]
pub fn lead_space_len(t: &[u16]) -> usize {
    t.iter().position(|&u| !uo_jsnum::text::is_js_space_unit(u)).unwrap_or(t.len())
}

/// Where `/\s*$/` matches first (the start of the trailing white space; the length of `t` when
/// there is none).
#[must_use]
pub fn trail_space_start(t: &[u16]) -> usize {
    t.iter().rposition(|&u| !uo_jsnum::text::is_js_space_unit(u)).map_or(0, |i| i + 1)
}

/// `s.replace(/\|/g, '\\|').replace(/\n/g, ' ')`.
#[must_use]
pub fn cell_text_escape(s: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    for &u in s {
        match u {
            0x7c => {
                out.push(0x5c);
                out.push(0x7c);
            }
            0x0a => out.push(0x20),
            _ => out.push(u),
        }
    }
    out
}

/// `s.replace(/\n{3,}/g, '\n\n')`.
#[must_use]
pub fn collapse_blank_lines(s: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s.get(i) == Some(&0x0a) {
            let mut j = i;
            while s.get(j) == Some(&0x0a) {
                j += 1;
            }
            let run = j - i;
            out.extend(std::iter::repeat_n(0x0a, if run >= 3 { 2 } else { run }));
            i = j;
        } else {
            if let Some(&u) = s.get(i) {
                out.push(u);
            }
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_levels() {
        assert_eq!(heading_level_of("Heading1"), 1);
        assert_eq!(heading_level_of("heading 3"), 3);
        assert_eq!(heading_level_of("Heading9"), 6);
        assert_eq!(heading_level_of("Título2"), 2);
        assert_eq!(heading_level_of("TÍTULO 4"), 4);
        assert_eq!(heading_level_of("titulo5"), 5);
        assert_eq!(heading_level_of("H3"), 3);
        assert_eq!(heading_level_of("h7"), 0);
        assert_eq!(heading_level_of("Title"), 1);
        assert_eq!(heading_level_of("Título"), 1);
        assert_eq!(heading_level_of("Heading"), 0);
        assert_eq!(heading_level_of("MyHeading2x"), 2);
        assert_eq!(heading_level_of("Heading0"), 0);
        assert_eq!(heading_level_of(""), 0);
    }

    #[test]
    fn collapses_only_three_or_more() {
        let u = |s: &str| s.encode_utf16().collect::<Vec<_>>();
        assert_eq!(collapse_blank_lines(&u("a\n\n\n\nb\n\nc\nd")), u("a\n\nb\n\nc\nd"));
    }

    #[test]
    fn trailing_space_start() {
        let u: Vec<u16> = " a b \u{a0}".encode_utf16().collect();
        assert_eq!(lead_space_len(&u), 1);
        assert_eq!(trail_space_start(&u), 4);
        let all: Vec<u16> = "  ".encode_utf16().collect();
        assert_eq!(lead_space_len(&all), 2);
        assert_eq!(trail_space_start(&all), 0);
    }
}
