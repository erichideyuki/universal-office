//! Entity and character reference decoding, with the end-of-line and attribute-value
//! normalization of the Sheet reader and of the strict mode.

use super::chars::{is_js_word, is_xml_char};
use crate::error::XmlErrorKind;

/// What the TS presets do with a numeric reference outside Unicode (or not a number).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharRefPolicy {
    /// error (docx-core and sheet-core: `String.fromCodePoint` throws a RangeError)
    Error,
    /// U+FFFD (slides-core `codePoint`)
    Replace,
}

/// Literal normalization applied before decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Norm {
    None,
    /// text and CDATA: CRLF and lone CR become LF (XML 1.0 §2.11)
    Eol,
    /// attribute values: CRLF counts once, TAB/LF/CR become a space (XML 1.0 §3.3.3)
    Attr,
}

/// Whether `raw` needs the decoder at all (else the source span is the value).
#[inline]
pub(crate) fn needs_decoding(raw: &[u8], norm: Norm) -> bool {
    match norm {
        Norm::None => raw.contains(&b'&'),
        Norm::Eol => raw.iter().any(|&c| c == b'&' || c == b'\r'),
        Norm::Attr => raw.iter().any(|&c| matches!(c, b'&' | b'\r' | b'\n' | b'\t')),
    }
}

const ENTITIES: [(&[u8], char); 5] = [(b"amp", '&'), (b"lt", '<'), (b"gt", '>'), (b"quot", '"'), (b"apos", '\'')];

/// The five predefined entities. Own table lookup: `&constructor;` stays literal (gate 1, G5;
/// sheet-core and slides-core `ENTITIES[e]` leaked `Object.prototype` members).
fn named(name: &[u8]) -> Option<char> {
    ENTITIES.iter().find(|(n, _)| *n == name).map(|&(_, c)| c)
}

/// Joins a high surrogate with the low surrogate of the NEXT reference, as a JS string does when
/// `&#xD83D;&#xDE00;` is decoded (decision in the record, skeptic #6); a lone half → U+FFFD.
struct Out<'o> {
    out: &'o mut String,
    high: Option<u32>,
}

impl Out<'_> {
    fn flush(&mut self) {
        if self.high.take().is_some() {
            self.out.push('\u{FFFD}');
        }
    }
    fn text(&mut self, s: &str) {
        if !s.is_empty() {
            self.flush();
            self.out.push_str(s);
        }
    }
    fn ch(&mut self, c: char) {
        self.flush();
        self.out.push(c);
    }
    fn code_point(&mut self, cp: u32) {
        match cp {
            0xD800..=0xDBFF => {
                self.flush();
                self.high = Some(cp);
            }
            0xDC00..=0xDFFF => match self.high.take() {
                Some(h) => {
                    let c = 0x1_0000 + ((h - 0xD800) << 10) + (cp - 0xDC00);
                    self.out.push(char::from_u32(c).unwrap_or('\u{FFFD}'));
                }
                None => self.out.push('\u{FFFD}'),
            },
            _ => self.ch(char::from_u32(cp).unwrap_or('\u{FFFD}')),
        }
    }
}

/// One match of the TS regex `/&(#x?[0-9a-fA-F]+|\w+);/` at `i` (which holds `&`).
enum RefMatch {
    /// `&#x<hex>;` (lowercase x only: `&#X41;` does not match and stays literal)
    Hex(usize, usize),
    /// `&#<hexdigits>;` read with `parseInt(…, 10)`
    Dec(usize, usize),
    /// `&\w+;`
    Name(usize, usize),
}

fn run(b: &[u8], from: usize, f: impl Fn(u8) -> bool) -> usize {
    let mut j = from;
    while j < b.len() && f(b[j]) {
        j += 1;
    }
    j
}

/// Returns the match and the index after the `;`.
fn match_ref(b: &[u8], i: usize) -> Option<(RefMatch, usize)> {
    if b.get(i + 1) == Some(&b'#') {
        let j = i + 2;
        // `x?` is greedy: try with the x first, then without (backtracking of the regex);
        // shorter hex runs cannot be followed by `;`, so only the full runs are tried
        if b.get(j) == Some(&b'x') {
            let e = run(b, j + 1, |c| c.is_ascii_hexdigit());
            if e > j + 1 && b.get(e) == Some(&b';') {
                return Some((RefMatch::Hex(j + 1, e), e + 1));
            }
        }
        let e = run(b, j, |c| c.is_ascii_hexdigit());
        if e > j && b.get(e) == Some(&b';') {
            return Some((RefMatch::Dec(j, e), e + 1));
        }
        // second alternative `\w+` cannot start at '#'
        return None;
    }
    let e = run(b, i + 1, is_js_word);
    if e > i + 1 && b.get(e) == Some(&b';') {
        return Some((RefMatch::Name(i + 1, e), e + 1));
    }
    None
}

/// `parseInt(s, 16)` on an all-hex token, saturating above U+10FFFF (`None` = not a code point).
fn hex_value(digits: &[u8]) -> Option<u32> {
    let mut v: u32 = 0;
    for &d in digits {
        let x = char::from(d).to_digit(16)?;
        v = v.checked_mul(16)?.checked_add(x)?;
        if v > 0x10_FFFF {
            return None;
        }
    }
    Some(v)
}

/// `parseInt(s, 10)` on a hex-digit token: the leading decimal digits (`&#1a;` → 1); NaN when
/// it starts with a letter (`&#a;`).
fn dec_value(digits: &[u8]) -> Option<u32> {
    let n = digits.iter().take_while(|c| c.is_ascii_digit()).count();
    if n == 0 {
        return None;
    }
    let mut v: u32 = 0;
    for &d in &digits[..n] {
        v = v.checked_mul(10)?.checked_add(u32::from(d - b'0'))?;
        if v > 0x10_FFFF {
            return None;
        }
    }
    Some(v)
}

/// Pushes `s` with the literal normalization.
fn push_norm(o: &mut Out<'_>, s: &str, norm: Norm, prev_cr: &mut bool) {
    if norm == Norm::None {
        o.text(s);
        return;
    }
    let mut start = 0;
    let b = s.as_bytes();
    for (k, &c) in b.iter().enumerate() {
        let lf_after_cr = c == b'\n' && *prev_cr;
        *prev_cr = c == b'\r';
        let rep = match (c, norm) {
            (b'\r', Norm::Eol) => Some('\n'),
            (b'\r' | b'\t', Norm::Attr) => Some(' '),
            (b'\n', Norm::Attr) if !lf_after_cr => Some(' '),
            (b'\n', _) if lf_after_cr => None,
            _ => continue,
        };
        o.text(&s[start..k]);
        if let Some(r) = rep {
            o.ch(r);
        }
        start = k + 1;
    }
    o.text(&s[start..]);
}

/// Lenient decoding (the TS `decodeEntities`, after `normalizeEol`/`normalizeAttr` when asked).
/// Appends to `out`; the error offset is relative to `raw`.
pub(crate) fn decode_lenient(raw: &str, norm: Norm, policy: CharRefPolicy, out: &mut String) -> Result<(), (usize, XmlErrorKind)> {
    let b = raw.as_bytes();
    let mut o = Out { out, high: None };
    let mut prev_cr = false;
    let mut lit = 0;
    let mut i = 0;
    while let Some(k) = b[i..].iter().position(|&c| c == b'&') {
        let amp = i + k;
        let Some((m, next)) = match_ref(b, amp) else {
            i = amp + 1;
            continue;
        };
        push_norm(&mut o, &raw[lit..amp], norm, &mut prev_cr);
        prev_cr = false;
        let cp = match m {
            RefMatch::Name(s, e) => {
                match named(&b[s..e]) {
                    Some(c) => o.ch(c),
                    // unknown names stay literal
                    None => o.text(&raw[amp..next]),
                }
                None
            }
            RefMatch::Hex(s, e) => Some(hex_value(&b[s..e])),
            RefMatch::Dec(s, e) => Some(dec_value(&b[s..e])),
        };
        match cp {
            Some(Some(cp)) => o.code_point(cp),
            Some(None) => match policy {
                CharRefPolicy::Error => return Err((amp, XmlErrorKind::InvalidCharRef)),
                CharRefPolicy::Replace => o.ch('\u{FFFD}'),
            },
            None => {}
        }
        lit = next;
        i = next;
    }
    push_norm(&mut o, &raw[lit..], norm, &mut prev_cr);
    o.flush();
    Ok(())
}

/// Strict decoding (what `XmlReader` accepts): the five predefined entities, `&#N;` and
/// `&#xH;` to legal XML characters; literal characters must be legal too; `<` is refused in
/// attribute values and `]]>` in text.
pub(crate) fn decode_strict(raw: &str, norm: Norm, is_attr: bool, out: &mut String) -> Result<(), (usize, XmlErrorKind)> {
    check_chars_strict(raw, is_attr)?;
    let b = raw.as_bytes();
    let mut o = Out { out, high: None };
    let mut prev_cr = false;
    let mut lit = 0;
    while let Some(k) = b[lit..].iter().position(|&c| c == b'&') {
        let amp = lit + k;
        let semi = b[amp..].iter().position(|&c| c == b';').map(|p| amp + p).ok_or((amp, XmlErrorKind::Unexpected))?;
        push_norm(&mut o, &raw[lit..amp], norm, &mut prev_cr);
        prev_cr = false;
        let body = &b[amp + 1..semi];
        if let Some(num) = body.strip_prefix(b"#") {
            let cp = if let Some(h) = num.strip_prefix(b"x") {
                if h.is_empty() || !h.iter().all(u8::is_ascii_hexdigit) {
                    return Err((amp, XmlErrorKind::Unexpected));
                }
                hex_value(h)
            } else {
                if num.is_empty() || !num.iter().all(u8::is_ascii_digit) {
                    return Err((amp, XmlErrorKind::Unexpected));
                }
                dec_value(num)
            };
            match cp {
                Some(cp) if is_xml_char(cp) => o.code_point(cp),
                _ => return Err((amp, XmlErrorKind::InvalidCharRef)),
            }
        } else {
            let name = std::str::from_utf8(body).map_err(|_| (amp, XmlErrorKind::Unexpected))?;
            if !super::chars::is_ncname(name) {
                return Err((amp, XmlErrorKind::Unexpected));
            }
            match named(body) {
                Some(c) => o.ch(c),
                None => return Err((amp, XmlErrorKind::UndeclaredEntity)),
            }
        }
        lit = semi + 1;
    }
    push_norm(&mut o, &raw[lit..], norm, &mut prev_cr);
    o.flush();
    Ok(())
}

/// Literal characters of text or an attribute value in strict mode.
pub(crate) fn check_chars_strict(raw: &str, is_attr: bool) -> Result<(), (usize, XmlErrorKind)> {
    let b = raw.as_bytes();
    for (k, &c) in b.iter().enumerate() {
        let bad = match c {
            0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F => true,
            b'<' => is_attr,
            b'>' => !is_attr && k >= 2 && b[k - 1] == b']' && b[k - 2] == b']',
            // U+FFFE, U+FFFF
            0xEF => b.get(k + 1) == Some(&0xBF) && matches!(b.get(k + 2), Some(0xBE | 0xBF)),
            _ => false,
        };
        if bad {
            let at = if c == b'>' { k - 2 } else { k };
            return Err((at, XmlErrorKind::Unexpected));
        }
    }
    Ok(())
}
