//! The regex sites of `numbering-store.ts` and `universal-write.ts` (lists), as hand matchers
//! (linear, no backtracking). Each function names the TS regex it stands for; the V8 rows are in
//! `js/regex-sites/d2b-f2.mjs` (replayed by `tests/regex_d2b_f2.rs`).
//!
//! All the sites are of one shape: `<name\b[^>]*\battr="value"[^>]*>` followed by a lazy
//! `[\s\S]*?</name>`. `[^>]*` cannot cross a `>`, so a tag is the text from `<name` to the first
//! `>`, and `\b` before the attribute means a non-word character right before it.

use uo_jsnum::text::is_js_word;

/// JS `\b` at byte `i` when the byte before it is a word character: true at the end of the text
/// and before a non-word byte.
pub(crate) fn boundary_after_word(xml: &str, i: usize) -> bool {
    xml.as_bytes().get(i).is_none_or(|b| !is_js_word(*b))
}

/// The next `<name` at or after `from` whose name is followed by `\b` (`<w:num\b` does not match
/// `<w:numFmt`).
pub(crate) fn next_open(xml: &str, name: &str, from: usize) -> Option<usize> {
    let pat = format!("<{name}");
    let mut at = from;
    while let Some(p) = xml.get(at..)?.find(&pat) {
        let s = at + p;
        if boundary_after_word(xml, s + pat.len()) {
            return Some(s);
        }
        at = s + 1;
    }
    None
}

/// The attribute text of the tag opened at `s` (`<name` then up to the first `>`): the range of
/// the text after the name and the position of the `>`.
fn tag_attrs(xml: &str, s: usize, name: &str) -> Option<(usize, usize)> {
    let a = s + 1 + name.len();
    let gt = a + xml.get(a..)?.find('>')?;
    Some((a, gt))
}

/// Whether `attrs` holds `\battr="value"` (a non-word byte right before it).
fn has_attr(attrs: &str, attr: &str, value: &str) -> bool {
    let needle = format!("{attr}=\"{value}\"");
    let mut at = 0;
    while let Some(p) = attrs.get(at..).and_then(|r| r.find(&needle)) {
        let q = at + p;
        // q = 0 cannot be a boundary: the byte before the attribute text is the last byte of the name
        if q > 0 && attrs.as_bytes().get(q - 1).is_some_and(|b| !is_js_word(*b)) {
            return true;
        }
        at = q + 1;
    }
    false
}

/// `<name\b[^>]*\battr="value"[^>]*>` at or after `from`: `(start, end after the '>')`.
pub(crate) fn tag_with_attr(xml: &str, name: &str, attr: &str, value: &str, from: usize) -> Option<(usize, usize)> {
    let mut at = from;
    loop {
        let s = next_open(xml, name, at)?;
        let (a, gt) = tag_attrs(xml, s, name)?;
        if has_attr(xml.get(a..gt)?, attr, value) {
            return Some((s, gt + 1));
        }
        at = s + 1;
    }
}

/// `<w:abstractNum\b[^>]*\bw:abstractNumId="ID"[^>]*>[\s\S]*?</w:abstractNum>`: the range of the
/// first match. A tag whose attribute matches but that has no closing tag after it ends the
/// search (no later start could close either).
pub(crate) fn element_range(xml: &str, name: &str, attr: &str, value: &str, close: &str) -> Option<(usize, usize)> {
    let (s, e) = tag_with_attr(xml, name, attr, value, 0)?;
    let c = xml.get(e..)?.find(close)?;
    Some((s, e + c + close.len()))
}

/// The `w:num` regex: `<w:num\b[^>]*\bw:numId="ID"[^>]*(?:/>|>[\s\S]*?</w:num>)`.
#[allow(dead_code)]
pub(crate) fn num_range(xml: &str, id: &str) -> Option<(usize, usize)> {
    let mut at = 0;
    loop {
        let (s, e) = tag_with_attr(xml, "w:num", "w:numId", id, at)?;
        // the greedy `[^>]*` stops at the '>': `>[\s\S]*?</w:num>` is tried before `/>` (an element
        // that closes itself still runs to a LATER `</w:num>`); only without one does the `[^>]*`
        // give back the '/' for `/>`
        if let Some(c) = xml.get(e..).and_then(|r| r.find("</w:num>")) {
            return Some((s, e + c + "</w:num>".len()));
        }
        if e >= 2 && xml.as_bytes().get(e - 2) == Some(&b'/') {
            return Some((s, e));
        }
        at = s + 1;
    }
}

/// `…\battr="(\d+)"` in the text of one tag (greedy `[^>]*` before it: the last occurrence):
/// the digits.
pub(crate) fn last_digits_attr<'a>(attrs: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{attr}=\"");
    let mut best: Option<&str> = None;
    let mut at = 0;
    while let Some(p) = attrs.get(at..).and_then(|r| r.find(&needle)) {
        let q = at + p;
        let vs = q + needle.len();
        let digits = attrs.get(vs..).map_or(0, |r| r.bytes().take_while(u8::is_ascii_digit).count());
        if q > 0
            && attrs.as_bytes().get(q - 1).is_some_and(|b| !is_js_word(*b))
            && digits > 0
            && attrs.as_bytes().get(vs + digits) == Some(&b'"')
        {
            best = attrs.get(vs..vs + digits);
        }
        at = q + 1;
    }
    best
}

/// `matchAll(/<name\b[^>]*\battr="(\d+)"/g)`: the digits of every match, in order.
pub(crate) fn all_digits_attr<'a>(xml: &'a str, name: &str, attr: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(s) = next_open(xml, name, at) {
        let Some((a, gt)) = tag_attrs(xml, s, name) else { break };
        if let Some(d) = xml.get(a..gt).and_then(|t| last_digits_attr(t, attr)) {
            out.push(d);
        }
        at = gt + 1;
    }
    out
}

/// `/\d+/` as a JS number from ASCII digits.
pub(crate) fn digits_to_f64(d: &str) -> f64 {
    uo_jsnum::parse::parse_int(d, 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn num_is_not_numfmt() {
        let x = "<w:numFmt w:val=\"x\"/><w:num w:numId=\"3\"><w:abstractNumId w:val=\"1\"/></w:num>";
        assert_eq!(num_range(x, "3").map(|(s, e)| &x[s..e]), Some(&x[x.find("<w:num ").unwrap_or(0)..]));
        assert_eq!(all_digits_attr(x, "w:num", "w:numId"), vec!["3"]);
    }

    #[test]
    fn abstract_by_id() {
        let x = "<w:abstractNum w:abstractNumId=\"0\"><w:lvl/></w:abstractNum><w:abstractNum w:abstractNumId=\"1\"></w:abstractNum>";
        assert_eq!(
            element_range(x, "w:abstractNum", "w:abstractNumId", "1", "</w:abstractNum>").map(|(s, e)| &x[s..e]),
            Some("<w:abstractNum w:abstractNumId=\"1\"></w:abstractNum>")
        );
    }
}
