//! Frontier R4c (D1a reader, record "Milestone 1 — D1a foundation"): `JSON.parse` as V8 for the
//! `wordnext:inline:` custom-inline tag (`document.ts` `parseCustomInlineTag`), and the
//! `decodeURIComponent` that precedes it (declarations in `core/docs/modules/uo-docx-d1a/R4c.md`).
//!
//! `JSON.parse` (ECMA-262 25.5.1 over ECMA-404): only space, TAB, LF and CR are white space;
//! numbers follow the strict grammar (`-0` is `-0`, an exponent past the range is `±Infinity`);
//! strings cannot hold a raw control character below U+0020; `\uXXXX` escapes may build lone
//! surrogates; a duplicate key keeps the first position and the last value; the integer-like
//! keys of an object come first in ascending order ([`JsRecord`]). The parser is iterative (an
//! explicit stack) and refuses nesting deeper than [`MAX_DEPTH`] (item 9 of the record: V8 has
//! no limit, the digest and the drops are recursive here).

use uo_jsnum::parse::to_number;

use crate::js::{JsRecord, JsonValue};
use crate::text::Utf16;

/// The deepest array/object nesting [`parse`] accepts.
pub const MAX_DEPTH: usize = 128;

/// `decodeURIComponent(s)`: every `%XX` sequence is decoded, the bytes of a multi-byte sequence
/// must form a valid UTF-8 encoding of a code point (no overlong form, no surrogate, nothing past
/// U+10FFFF); anything else (`%` without two hex digits, a truncated or invalid sequence) is the
/// `URIError` the TS catches: `None`.
#[must_use]
pub fn decode_uri_component(s: &str) -> Option<String> {
    if !s.contains('%') {
        return Some(s.to_owned());
    }
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b.get(i) != Some(&b'%') {
            let rest = s.get(i..)?;
            let c = rest.chars().next()?;
            out.push(c);
            i += c.len_utf8();
            continue;
        }
        let first = hex_byte(b, i)?;
        i += 3;
        if first < 0x80 {
            out.push(char::from(first));
            continue;
        }
        let n = match first {
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => return None,
        };
        let mut bytes = vec![first];
        for _ in 1..n {
            if b.get(i) != Some(&b'%') {
                return None;
            }
            let k = hex_byte(b, i)?;
            if k & 0xC0 != 0x80 {
                return None;
            }
            bytes.push(k);
            i += 3;
        }
        out.push_str(std::str::from_utf8(&bytes).ok()?);
    }
    Some(out)
}

/// The byte of `%XX` at `i` (`None` when it is not two hex digits).
fn hex_byte(b: &[u8], i: usize) -> Option<u8> {
    let h = hex_digit(*b.get(i + 1)?)?;
    let l = hex_digit(*b.get(i + 2)?)?;
    Some(h << 4 | l)
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// An open array or object of the parse.
enum Open {
    Array(Vec<JsonValue>),
    /// the members so far and the key waiting for its value
    Object(JsRecord<JsonValue>, Option<Box<str>>),
}

/// What the parser expects next inside an open container.
#[derive(Clone, Copy, PartialEq, Eq)]
enum After {
    /// the first element or `]` / the first key or `}`
    First,
    /// `,` or the closing bracket
    Next,
    /// a value (after `,` or `:`)
    Value,
}

struct Parser<'a> {
    s: &'a str,
    b: &'a [u8],
    pos: usize,
}

/// `JSON.parse(text)`: `None` is the `SyntaxError` (and the nesting past [`MAX_DEPTH`]).
#[must_use]
pub fn parse(text: &str) -> Option<JsonValue> {
    let mut p = Parser { s: text, b: text.as_bytes(), pos: 0 };
    p.parse_document()
}

impl Parser<'_> {
    fn ws(&mut self) {
        while matches!(self.b.get(self.pos), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    fn lit(&mut self, word: &str) -> bool {
        if self.s.get(self.pos..).is_some_and(|r| r.starts_with(word)) {
            self.pos += word.len();
            true
        } else {
            false
        }
    }

    fn parse_document(&mut self) -> Option<JsonValue> {
        let mut stack: Vec<Open> = Vec::new();
        let mut state = After::Value;
        let done = loop {
            self.ws();
            let mut value: Option<JsonValue> = None;
            match (stack.last_mut(), state) {
                (Some(Open::Array(_)), After::First) if self.peek() == Some(b']') => {
                    self.pos += 1;
                    if let Some(Open::Array(a)) = stack.pop() {
                        value = Some(JsonValue::Array(a));
                    }
                }
                (Some(Open::Array(_)), After::Next) => match self.peek()? {
                    b',' => {
                        self.pos += 1;
                        state = After::Value;
                        continue;
                    }
                    b']' => {
                        self.pos += 1;
                        if let Some(Open::Array(a)) = stack.pop() {
                            value = Some(JsonValue::Array(a));
                        }
                    }
                    _ => return None,
                },
                (Some(Open::Object(..)), After::First) if self.peek() == Some(b'}') => {
                    self.pos += 1;
                    if let Some(Open::Object(o, _)) = stack.pop() {
                        value = Some(JsonValue::Object(o));
                    }
                }
                (Some(Open::Object(..)), After::Next) => match self.peek()? {
                    b',' => {
                        self.pos += 1;
                        self.ws();
                        self.key(stack.last_mut())?;
                        state = After::Value;
                        continue;
                    }
                    b'}' => {
                        self.pos += 1;
                        if let Some(Open::Object(o, _)) = stack.pop() {
                            value = Some(JsonValue::Object(o));
                        }
                    }
                    _ => return None,
                },
                (Some(Open::Object(..)), After::First) => {
                    self.key(stack.last_mut())?;
                    state = After::Value;
                    continue;
                }
                _ => {
                    // a value
                    match self.peek()? {
                        b'[' => {
                            if stack.len() >= MAX_DEPTH {
                                return None;
                            }
                            self.pos += 1;
                            stack.push(Open::Array(Vec::new()));
                            state = After::First;
                            continue;
                        }
                        b'{' => {
                            if stack.len() >= MAX_DEPTH {
                                return None;
                            }
                            self.pos += 1;
                            stack.push(Open::Object(JsRecord::new(), None));
                            state = After::First;
                            continue;
                        }
                        b'"' => value = Some(JsonValue::Str(self.string()?)),
                        b't' if self.lit("true") => value = Some(JsonValue::Bool(true)),
                        b'f' if self.lit("false") => value = Some(JsonValue::Bool(false)),
                        b'n' if self.lit("null") => value = Some(JsonValue::Null),
                        b'-' | b'0'..=b'9' => value = Some(JsonValue::Num(self.number()?)),
                        _ => return None,
                    }
                }
            }
            let Some(v) = value else { continue };
            match stack.last_mut() {
                None => break v,
                Some(Open::Array(a)) => {
                    a.push(v);
                    state = After::Next;
                }
                Some(Open::Object(o, key)) => {
                    let k = key.take()?;
                    o.insert(&k, v);
                    state = After::Next;
                }
            }
        };
        self.ws();
        if self.pos != self.b.len() {
            return None;
        }
        Some(done)
    }

    /// `"key" :` of an object member, stored as the pending key of the open object.
    fn key(&mut self, top: Option<&mut Open>) -> Option<()> {
        if self.peek()? != b'"' {
            return None;
        }
        let k = self.string()?;
        self.ws();
        if self.peek()? != b':' {
            return None;
        }
        self.pos += 1;
        if let Some(Open::Object(_, slot)) = top {
            *slot = Some(k.to_string_lossy().into());
        }
        Some(())
    }

    /// A string literal at the cursor (the opening quote included).
    fn string(&mut self) -> Option<Utf16> {
        self.pos += 1;
        let mut units: Vec<u16> = Vec::new();
        loop {
            let rest = self.s.get(self.pos..)?;
            let c = rest.chars().next()?;
            match c {
                '"' => {
                    self.pos += 1;
                    return Some(Utf16::from_units(units));
                }
                '\\' => {
                    self.pos += 1;
                    let e = *self.b.get(self.pos)?;
                    self.pos += 1;
                    match e {
                        b'"' => units.push(0x22),
                        b'\\' => units.push(0x5c),
                        b'/' => units.push(0x2f),
                        b'b' => units.push(8),
                        b'f' => units.push(12),
                        b'n' => units.push(10),
                        b'r' => units.push(13),
                        b't' => units.push(9),
                        b'u' => {
                            let mut v: u16 = 0;
                            for _ in 0..4 {
                                let d = hex_digit(*self.b.get(self.pos)?)?;
                                v = v << 4 | u16::from(d);
                                self.pos += 1;
                            }
                            units.push(v);
                        }
                        _ => return None,
                    }
                }
                c if (c as u32) < 0x20 => return None,
                c => {
                    let mut buf = [0u16; 2];
                    units.extend_from_slice(c.encode_utf16(&mut buf));
                    self.pos += c.len_utf8();
                }
            }
        }
    }

    /// A number literal of the strict JSON grammar at the cursor.
    fn number(&mut self) -> Option<f64> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek()? {
            b'0' => self.pos += 1,
            b'1'..=b'9' => self.digits(),
            _ => return None,
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            self.digits1()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            self.digits1()?;
        }
        Some(to_number(self.s.get(start..self.pos)?))
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
    }

    fn digits1(&mut self) -> Option<()> {
        if !matches!(self.peek(), Some(b'0'..=b'9')) {
            return None;
        }
        self.digits();
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(v: &JsonValue) -> String {
        use crate::js::{JsValue, JsonWriter};
        let mut w = JsonWriter::new(None);
        v.json(&mut w);
        w.finish()
    }

    #[test]
    fn parses_like_json_parse() {
        let p = |s: &str| parse(s).map(|v| json(&v));
        assert_eq!(p(" {\"a\": [1, 2.5e1, -0, true, null, \"x\\u00e9\"]} ").as_deref(), Some("{\"a\":[1,25,0,true,null,\"xé\"]}"));
        assert_eq!(p("{\"b\":1,\"a\":2,\"b\":3}").as_deref(), Some("{\"b\":3,\"a\":2}"));
        assert_eq!(p("{\"b\":1,\"2\":2,\"1\":3}").as_deref(), Some("{\"1\":3,\"2\":2,\"b\":1}"));
        assert_eq!(p("1e999").as_deref(), Some("null"));
        assert!(p("01").is_none() && p("1.").is_none() && p(".5").is_none() && p("+1").is_none());
        assert!(p("[1,]").is_none() && p("{\"a\":1,}").is_none() && p("{a:1}").is_none() && p("'a'").is_none());
        assert!(p("").is_none() && p("[").is_none() && p("[1 2]").is_none() && p("1 2").is_none());
        assert!(p("\"a\nb\"").is_none() && p("\"\\x\"").is_none() && p("\"\\u12\"").is_none());
        assert!(p("\u{a0}1").is_none() && p("\u{feff}1").is_none());
        assert_eq!(p("\t\r\n1").as_deref(), Some("1"));
        assert_eq!(p("\"\\ud800\"").as_deref(), Some("\"\\ud800\""));
    }

    #[test]
    fn negative_zero_and_depth() {
        assert!(matches!(parse("-0"), Some(JsonValue::Num(x)) if x == 0.0 && x.is_sign_negative()));
        let deep = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
        assert!(parse(&deep(MAX_DEPTH)).is_some());
        assert!(parse(&deep(MAX_DEPTH + 1)).is_none());
        let wide = format!("{}1{}", "{\"a\":".repeat(MAX_DEPTH), "}".repeat(MAX_DEPTH));
        assert!(parse(&wide).is_some());
    }

    #[test]
    fn decodes_uri_components() {
        assert_eq!(decode_uri_component("a%20b%7B%22k%22%3A1%7D").as_deref(), Some("a b{\"k\":1}"));
        assert_eq!(decode_uri_component("%E2%82%AC é").as_deref(), Some("€ é"));
        assert_eq!(decode_uri_component("%f0%9f%98%80").as_deref(), Some("😀"));
        assert!(decode_uri_component("%").is_none());
        assert!(decode_uri_component("%2").is_none());
        assert!(decode_uri_component("%zz").is_none());
        assert!(decode_uri_component("%E2%82").is_none());
        assert!(decode_uri_component("%80").is_none());
        assert!(decode_uri_component("%C0%80").is_none());
        assert!(decode_uri_component("%ED%A0%80").is_none());
        assert!(decode_uri_component("%F8%88%80%80%80").is_none());
        assert!(decode_uri_component("%E2%28%A1").is_none());
        assert!(decode_uri_component("%F4%90%80%80").is_none());
    }
}
