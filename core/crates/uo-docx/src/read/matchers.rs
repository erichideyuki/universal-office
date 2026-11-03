//! Hand-written matchers for the reader's regular expressions (G9: no regex engine). Each TS
//! regex site of a frontier becomes a call to these combinators, written by the **per-site
//! template** below and listed as a row of the frontier's spec table in the record; the
//! templates themselves are differential-tested against V8 `RegExp` (`js/regex-sites/templates.mjs`
//! run by `js/regex-diff.mjs` → `tests/regex/templates.expected`, test `regex_templates`).
//!
//! Semantics (JS `RegExp` without the `u` flag, the only kind in the reader sites except where a
//! row says otherwise):
//! - `\s` is the JS white space set ([`uo_jsnum::text::is_js_space`]); `\d` is `[0-9]`; `\w` is
//!   `[A-Za-z0-9_]`; `\b` is a boundary between `\w` and not-`\w` (string ends count as
//!   not-`\w`).
//! - `i` without `u`: ASCII letters match case-insensitively and nothing else does (a non-ASCII
//!   character never canonicalizes to an ASCII one: ECMA-262 Canonicalize).
//! - Positions are byte offsets of the UTF-8 string; the patterns only contain ASCII, so a match
//!   starts and ends at the same characters as in the UTF-16 string (convert offsets with the
//!   string only when a position leaves the site).
//!
//! **Per-site template** (one row per site in the frontier's table: `file:line | TS regex |
//! Rust | note`):
//! 1. Anchored alternation of literals `/^(a|b)$/.test(x)` → [`is_one_of`].
//! 2. Unanchored literal with boundaries `/\bTOC\b/.test(x)`, `/<a:blipFill\b/.test(x)` →
//!    [`find_word`] (`ci` for `/i`), [`find_lit_then_boundary`].
//! 3. Prefix/suffix `replace(/^#/, '')`, `/x$/.test(s)` → `strip_prefix`/`ends_with` (std; a
//!    `$` in a non-`m` regex is the string end only).
//! 4. Class filters `replace(/\D/g, '')`, `/^[a-zA-Z0-9._-]+$/` → [`keep_ascii_digits`],
//!    [`all_in`].
//! 5. Sequences (`/^\s*TOC\b/i`, `/^val\s+(-?\d+)/`, `measureTwips`'s
//!    `/^(-?[0-9.]+)(mm|cm|in|pt|pc|pi)?$/`) → a [`Cur`] walk: `lit`, `lit_ci`, `spaces`,
//!    `spaces1`, `opt`, `take_while`, `boundary`, `end`.
//! 6. Global replaces of fixed strings (`/­/g`, `/\r\n|[\r\n]/g`, `/\s*\/>$/`) →
//!    [`replace_char`], [`newlines_to_space`], [`self_closing_to_open`].
//! 7. Dynamic patterns built from a constant (`new RegExp('<' + tag + '[^>]*>([^<]*)<')`) →
//!    [`element_text`]; built from document data (`Id="${rid}"`) → [`find_attr_literal`],
//!    the value matched as a literal (G9: the TS metacharacter and `SyntaxError` behaviour is
//!    accepted by name; the row says so).
//! 8. Anything else: a [`Cur`] walk written for the site. Every site of a frontier (templates
//!    included) gets a differential row in its own `js/regex-sites/<frontier>.mjs` and Rust test
//!    `tests/regex_<frontier>.rs`; backtracking patterns get a work budget.

use uo_jsnum::text::is_js_space;

/// `\w` of JS (ASCII).
#[must_use]
pub fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// A cursor over a string for sequence patterns (template 5).
#[derive(Debug, Clone, Copy)]
pub struct Cur<'a> {
    s: &'a str,
    pos: usize,
}

impl<'a> Cur<'a> {
    #[must_use]
    pub fn new(s: &'a str) -> Cur<'a> {
        Cur { s, pos: 0 }
    }

    /// At byte `pos` of `s`.
    #[must_use]
    pub fn at(s: &'a str, pos: usize) -> Cur<'a> {
        Cur { s, pos: pos.min(s.len()) }
    }

    #[must_use]
    pub fn pos(&self) -> usize {
        self.pos
    }

    #[must_use]
    pub fn rest(&self) -> &'a str {
        self.s.get(self.pos..).unwrap_or("")
    }

    /// `$` (no `m` flag): at the end.
    #[must_use]
    pub fn end(&self) -> bool {
        self.pos >= self.s.len()
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    /// The character before the cursor.
    fn prev(&self) -> Option<char> {
        self.s.get(..self.pos).and_then(|h| h.chars().next_back())
    }

    /// A literal.
    pub fn lit(&mut self, l: &str) -> bool {
        if self.rest().starts_with(l) {
            self.pos += l.len();
            true
        } else {
            false
        }
    }

    /// A literal under `/i` without `u` (ASCII case folding only).
    pub fn lit_ci(&mut self, l: &str) -> bool {
        match self.rest().get(..l.len()) {
            Some(h) if h.eq_ignore_ascii_case(l) => {
                self.pos += l.len();
                true
            }
            _ => false,
        }
    }

    /// `c?`.
    pub fn opt(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    /// `[class]*` (greedy): the matched text.
    pub fn take_while(&mut self, f: impl Fn(char) -> bool) -> &'a str {
        let start = self.pos;
        while let Some(c) = self.peek().filter(|&c| f(c)) {
            self.pos += c.len_utf8();
        }
        self.s.get(start..self.pos).unwrap_or("")
    }

    /// `[class]+`: `None` (cursor unchanged) when nothing matches.
    pub fn take_while1(&mut self, f: impl Fn(char) -> bool) -> Option<&'a str> {
        let t = self.take_while(f);
        (!t.is_empty()).then_some(t)
    }

    /// `\s*`.
    pub fn spaces(&mut self) -> usize {
        self.take_while(is_js_space).len()
    }

    /// `\s+`.
    pub fn spaces1(&mut self) -> bool {
        self.spaces() > 0
    }

    /// `\d+` (the digits).
    pub fn digits1(&mut self) -> Option<&'a str> {
        self.take_while1(|c| c.is_ascii_digit())
    }

    /// `\b` at the cursor.
    #[must_use]
    pub fn boundary(&self) -> bool {
        self.prev().is_some_and(is_word) != self.peek().is_some_and(is_word)
    }
}

/// Template 1: `/^(a|b|…)$/.test(s)`.
#[must_use]
pub fn is_one_of(s: &str, alts: &[&str]) -> bool {
    alts.contains(&s)
}

/// Template 2: the first match of `/\bword\b/` (`ci`: `/\bword\b/i`), as a byte offset. `word`
/// starts and ends with `\w` characters.
#[must_use]
pub fn find_word(hay: &str, word: &str, ci: bool) -> Option<usize> {
    let mut from = 0;
    while from <= hay.len() {
        let rest = hay.get(from..)?;
        let k = if ci {
            rest.as_bytes().windows(word.len().max(1)).position(|w| w.eq_ignore_ascii_case(word.as_bytes()))?
        } else {
            rest.find(word)?
        };
        let at = from + k;
        let before = Cur::at(hay, at);
        let after = Cur::at(hay, at + word.len());
        if before.boundary() && after.boundary() {
            return Some(at);
        }
        from = at + hay.get(at..)?.chars().next()?.len_utf8();
    }
    None
}

/// Template 2: the first match of `/lit\b/` where `lit` ends with a `\w` character (`<a:blipFill\b`).
#[must_use]
pub fn find_lit_then_boundary(hay: &str, lit: &str) -> Option<usize> {
    let mut from = 0;
    loop {
        let at = from + hay.get(from..)?.find(lit)?;
        if Cur::at(hay, at + lit.len()).boundary() {
            return Some(at);
        }
        from = at + hay.get(at..)?.chars().next()?.len_utf8();
    }
}

/// Template 4: `s.replace(/\D/g, '')`.
#[must_use]
pub fn keep_ascii_digits(s: &str) -> String {
    s.chars().filter(char::is_ascii_digit).collect()
}

/// Template 4: `/^[class]+$/.test(s)` (non-empty, every character in the class).
#[must_use]
pub fn all_in(s: &str, f: impl Fn(char) -> bool) -> bool {
    !s.is_empty() && s.chars().all(f)
}

/// Template 6: `s.replace(/c/g, to)` for one character.
#[must_use]
pub fn replace_char(s: &str, from: char, to: char) -> String {
    s.chars().map(|c| if c == from { to } else { c }).collect()
}

/// Template 6: `s.replace(/\r\n|[\r\n]/g, ' ')`.
#[must_use]
pub fn newlines_to_space(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\r' => {
                if it.peek() == Some(&'\n') {
                    it.next();
                }
                out.push(' ');
            }
            '\n' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Template 6: `s.replace(/\s*\/>$/, '>')`: a self-closing tag's end made an open tag's end.
#[must_use]
pub fn self_closing_to_open(s: &str) -> std::borrow::Cow<'_, str> {
    let Some(head) = s.strip_suffix("/>") else { return s.into() };
    let trimmed = head.trim_end_matches(is_js_space);
    format!("{trimmed}>").into()
}

/// Template 7 (dynamic, from a constant): the first match of
/// `new RegExp('<' + tag + '[^>]*>([^<]*)<')`, its group 1. `tag` has no regex metacharacters
/// (the call sites pass constants such as `dcterms:created`).
#[must_use]
pub fn element_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let mut from = 0;
    loop {
        let rest = xml.get(from..)?;
        let k = rest.find('<')?;
        let at = from + k;
        let mut c = Cur::at(xml, at + 1);
        if c.lit(tag) {
            c.take_while(|ch| ch != '>');
            if c.opt('>') {
                let text = c.take_while(|ch| ch != '<');
                if c.opt('<') {
                    return Some(text);
                }
            }
        }
        from = at + 1;
    }
}

/// Template 7 (dynamic, from document data): the first occurrence of `name="value"` with
/// `value` matched as a literal (the TS interpolates it into a regex unescaped; its
/// metacharacter and `SyntaxError` behaviour is accepted by name, G9).
#[must_use]
pub fn find_attr_literal(hay: &str, name: &str, value: &str) -> Option<usize> {
    let pat = format!("{name}=\"{value}\"");
    hay.find(&pat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_follow_js() {
        assert!(is_one_of("true", &["1", "true"]));
        assert!(!is_one_of("TRUE", &["1", "true"]));
        assert_eq!(find_word(" TOC \\o", "TOC", false), Some(1));
        assert_eq!(find_word("xTOC TOC", "TOC", false), Some(5));
        assert_eq!(find_word("TOCx", "TOC", false), None);
        assert_eq!(find_word("é toc", "TOC", true), Some(3));
        assert_eq!(find_lit_then_boundary("<a:blipFillx/><a:blipFill r:embed", "<a:blipFill"), Some(14));
        assert_eq!(keep_ascii_digits("_x0000_s1026"), "00001026");
        assert!(all_in("pic.a-b_1", |c| c.is_ascii_alphanumeric() || "._-".contains(c)));
        assert!(!all_in("", |_| true));
        assert_eq!(newlines_to_space("a\r\nb\rc\nd\n\r"), "a b c d  ");
        assert_eq!(self_closing_to_open("<w:p w14:paraId=\"1\" \t/>"), "<w:p w14:paraId=\"1\">");
        assert_eq!(self_closing_to_open("<w:p>"), "<w:p>");
        assert_eq!(
            element_text("<cp:x/><dcterms:created xsi:type=\"W3CDTF\">2024-01-01T00:00:00Z</dcterms:created>", "dcterms:created"),
            Some("2024-01-01T00:00:00Z")
        );
        assert_eq!(element_text("<dcterms:createdX>v<", "dcterms:created"), Some("v"));
        let mut c = Cur::new("  toc \\h");
        c.spaces();
        assert!(c.lit_ci("TOC") && c.boundary());
        let mut v = Cur::new("val -2500");
        assert!(v.lit("val") && v.spaces1());
        let neg = v.opt('-');
        assert_eq!((neg, v.digits1()), (true, Some("2500")));
    }
}
