//! The JavaScript `\s` set over UTF-16: `trim`, `split(/\s+/)` and `replace(/\s+/g, ...)` on code units,
//! for text that may hold lone surrogates (so a `&str` is not enough: `uo_jsnum::text` has the UTF-8
//! versions, and its `is_js_space_unit` is the single definition of the set).
//!
//! `\s` is TAB, LF, VT, FF, CR, SPACE, U+00A0, U+1680, U+2000..U+200A, U+2028, U+2029, U+202F, U+205F,
//! U+3000 and U+FEFF; U+0085 and U+180E are not in it.

use std::borrow::Cow;

pub use uo_jsnum::text::is_js_space_unit as is_space;

/// `s.trim()`, `s.trimStart()` and `s.trimEnd()` use the same set.
#[must_use]
pub fn trim(units: &[u16]) -> &[u16] {
    trim_end(trim_start(units))
}

/// `s.trimStart()`.
#[must_use]
pub fn trim_start(units: &[u16]) -> &[u16] {
    let n = units.iter().take_while(|&&u| is_space(u)).count();
    units.get(n..).unwrap_or(&[])
}

/// `s.trimEnd()`.
#[must_use]
pub fn trim_end(units: &[u16]) -> &[u16] {
    let n = units.iter().rev().take_while(|&&u| is_space(u)).count();
    units.get(..units.len() - n).unwrap_or(&[])
}

/// The pieces of `s.split(/\s+/)`: a run of spaces separates, and a leading or trailing run leaves an empty first or
/// last piece (`" a b "` -> `""`, `"a"`, `"b"`, `""`); the empty string gives one empty piece.
#[derive(Debug, Clone)]
pub struct SplitSpaces<'a> {
    rest: Option<&'a [u16]>,
}

/// `s.split(/\s+/)`.
#[must_use]
pub fn split_spaces(units: &[u16]) -> SplitSpaces<'_> {
    SplitSpaces { rest: Some(units) }
}

impl<'a> Iterator for SplitSpaces<'a> {
    type Item = &'a [u16];

    fn next(&mut self) -> Option<&'a [u16]> {
        let rest = self.rest?;
        let Some(start) = rest.iter().position(|&u| is_space(u)) else {
            self.rest = None;
            return Some(rest);
        };
        let after = rest.get(start..).unwrap_or(&[]);
        self.rest = Some(trim_start(after));
        Some(rest.get(..start).unwrap_or(&[]))
    }
}

/// `s.replace(/\s/g, replacement)`: every space unit becomes `replacement` (a literal, as the call sites
/// pass; JS would read `$&` and friends in it). Borrows when there is no space.
#[must_use]
pub fn replace_each<'a>(units: &'a [u16], replacement: &[u16]) -> Cow<'a, [u16]> {
    replace(units, replacement, false)
}

/// `s.replace(/\s+/g, replacement)`: every run of spaces becomes `replacement` (a literal). Borrows when
/// there is no space.
#[must_use]
pub fn replace_runs<'a>(units: &'a [u16], replacement: &[u16]) -> Cow<'a, [u16]> {
    replace(units, replacement, true)
}

fn replace<'a>(units: &'a [u16], replacement: &[u16], runs: bool) -> Cow<'a, [u16]> {
    if !units.iter().any(|&u| is_space(u)) {
        return Cow::Borrowed(units);
    }
    let mut out = Vec::with_capacity(units.len());
    let mut in_run = false;
    for &u in units {
        if is_space(u) {
            if !(runs && in_run) {
                out.extend_from_slice(replacement);
            }
            in_run = true;
        } else {
            in_run = false;
            out.push(u);
        }
    }
    Cow::Owned(out)
}
