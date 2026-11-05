//! Field instructions: the hand matchers of the regular expressions the TS writer applies to
//! them (declaration 3 of D4: spec-table rows, linear, no engine), over UTF-16 units where `\s` is
//! the JavaScript space set and `i` folds ASCII letters only (a non-ASCII letter never matches an
//! ASCII one without the `u` flag).

use uo_text::space::is_space;

/// The part of a field instruction `HYPERLINK` found in a paragraph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HyperlinkField {
    /// the CP of the separator (`0x14`) of the field
    pub(super) sep: i64,
    /// the CP of the end of the field (`0x15`)
    pub(super) end: i64,
    pub(super) rid: Option<String>,
    pub(super) anchor: Option<Vec<u16>>,
    pub(super) tooltip: Option<Vec<u16>>,
    pub(super) target: Option<Vec<u16>>,
    /// a `SHAPE` field: the Word drops the field and keeps its content
    pub(super) unwrap: bool,
}

/// `\w` (no `u` flag): `[A-Za-z0-9_]`.
fn is_word_unit(u: u16) -> bool {
    matches!(u, 0x30..=0x39 | 0x41..=0x5a | 0x61..=0x7a | 0x5f)
}

/// A JavaScript line terminator (what `.` does not match).
fn is_line_terminator(u: u16) -> bool {
    matches!(u, 0x0a | 0x0d | 0x2028 | 0x2029)
}

fn upper(u: u16) -> u16 {
    if (0x61..=0x7a).contains(&u) { u - 32 } else { u }
}

/// The index after `word` (ASCII upper case) matched case-insensitively at `from`.
fn match_ci(units: &[u16], from: usize, word: &str) -> Option<usize> {
    let mut i = from;
    for b in word.bytes() {
        if upper(*units.get(i)?) != u16::from(b) {
            return None;
        }
        i += 1;
    }
    Some(i)
}

/// `\s*` from `from`: the index after the spaces.
fn skip_spaces(units: &[u16], from: usize) -> usize {
    let mut i = from;
    while units.get(i).copied().is_some_and(is_space) {
        i += 1;
    }
    i
}

/// `\b` after a word character: the next unit is not a word character, or there is none.
fn word_end_at(units: &[u16], i: usize) -> bool {
    !units.get(i).copied().is_some_and(is_word_unit)
}

/// `/^\s*SHAPE\b/i.test(instr)`.
pub(super) fn is_shape_field(instr: &[u16]) -> bool {
    match_ci(instr, skip_spaces(instr, 0), "SHAPE").is_some_and(|i| word_end_at(instr, i))
}

/// `/^\s*FORM(TEXT|CHECKBOX|DROPDOWN)\b/i.test(instr)`.
pub(super) fn is_form_field(instr: &[u16]) -> bool {
    let Some(i) = match_ci(instr, skip_spaces(instr, 0), "FORM") else { return false };
    ["TEXT", "CHECKBOX", "DROPDOWN"].iter().any(|kind| match_ci(instr, i, kind).is_some_and(|j| word_end_at(instr, j)))
}

/// `/^\s*HYPERLINK\s+(.*)$/i.exec(instr)?.[1]`: the arguments. `.` stops at a line terminator, and
/// `$` is the end of the text, so a terminator in the arguments is no match.
pub(super) fn hyperlink_args(instr: &[u16]) -> Option<&[u16]> {
    let j = match_ci(instr, skip_spaces(instr, 0), "HYPERLINK")?;
    let k = skip_spaces(instr, j);
    if k == j {
        return None;
    }
    let rest = instr.get(k..)?;
    if rest.iter().any(|&u| is_line_terminator(u)) { None } else { Some(rest) }
}

/// The end (the index of the closing quote) of the quoted argument that opens at `open`:
/// `"((?:[^"\\]|\\.)*)"`.
fn closing_quote(s: &[u16], open: usize) -> Option<usize> {
    let mut j = open + 1;
    loop {
        let c = *s.get(j)?;
        if c == 0x22 {
            return Some(j);
        }
        if c == 0x5c {
            // a backslash and the character after it (not a line terminator)
            let next = *s.get(j + 1)?;
            if is_line_terminator(next) {
                return None;
            }
            j += 2;
        } else {
            j += 1;
        }
    }
}

/// `parseFieldArgs`: the arguments of a field code, "quoted" (with `\x` as `x`) or words; `\x`
/// switches are words too.
pub(super) fn parse_field_args(s: &[u16]) -> Vec<Vec<u16>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] == 0x22
            && let Some(close) = closing_quote(s, i)
        {
            let content = &s[i + 1..close];
            let mut text = Vec::with_capacity(content.len());
            let mut k = 0;
            while k < content.len() {
                if content[k] == 0x5c && k + 1 < content.len() {
                    text.push(content[k + 1]);
                    k += 2;
                } else {
                    text.push(content[k]);
                    k += 1;
                }
            }
            out.push(text);
            i = close + 1;
            continue;
        }
        if !is_space(s[i]) {
            let mut j = i;
            while j < s.len() && !is_space(s[j]) {
                j += 1;
            }
            out.push(s[i..j].to_vec());
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn shape_and_form_fields() {
        assert!(is_shape_field(&u("  shape \\* MERGEFORMAT")));
        assert!(is_shape_field(&u("SHAPE")));
        assert!(!is_shape_field(&u("SHAPES")));
        assert!(!is_shape_field(&u("SHAPE_1")));
        assert!(is_form_field(&u(" FORMTEXT ")));
        assert!(is_form_field(&u("formdropdown")));
        assert!(!is_form_field(&u("FORMTEXTS")));
        assert!(!is_form_field(&u("FORMULA")));
        // the long s and the Kelvin sign fold to ASCII letters only with the `u` flag
        assert!(!is_shape_field(&[0x17f, 0x48, 0x41, 0x50, 0x45]));
    }

    #[test]
    fn hyperlink_arguments() {
        let i = u("HYPERLINK \"http://x/a b\" \\l \"top\"");
        assert_eq!(hyperlink_args(&i).map(<[u16]>::to_vec), Some(u("\"http://x/a b\" \\l \"top\"")));
        assert_eq!(hyperlink_args(&u("HYPERLINK")), None);
        assert_eq!(hyperlink_args(&u("HYPERLINKX a")), None);
        assert_eq!(hyperlink_args(&u("HYPERLINK a\nb")), None);
        assert_eq!(hyperlink_args(&u("HYPERLINK \n a")).map(<[u16]>::to_vec), Some(u("a")));
        let args = parse_field_args(&u("\"a\\\"b\" \\l top \"unterminated x"));
        assert_eq!(args, vec![u("a\"b"), u("\\l"), u("top"), u("\"unterminated"), u("x")]);
    }
}
