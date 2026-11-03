//! `AdvanceTable`: the empirical advances of the reference application by size
//! (`SfntFont.setAdvanceTable`, sfnt.ts:386-403), attached to a face when the host pushes it
//! (PQ1, LQ8(e)). Only the PDF word-space insertion reads it (`advanceTwips`); the layout's width
//! model never does.
//!
//! The TS shape is `{ "11": { "a": twips, ... } }`: a size as the JS string of the number
//! (`String(sizePt)`), then one entry per character. Sizes stay the JSON strings and are matched
//! against `to_js_string(sizePt)` by the caller; a character key counts only when it is exactly
//! one code point (`String.fromCodePoint(cp)` can equal no other key), a lone surrogate included.
//! A value that is not a number is dropped (the TS would hand the garbage to the caller).

use rustc_hash::FxHashMap;
use uo_jsnum::parse::to_number;

use crate::mem::table_bytes;

/// The hosted tables are a few KB; the input cap is a guard, not a limit anyone meets.
pub const MAX_ADVANCE_JSON_BYTES: usize = 8 << 20;
/// Entries (sizes plus characters) kept at most.
pub const MAX_ADVANCE_ENTRIES: usize = 1 << 20;
const MAX_DEPTH: u32 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvanceTableError {
    /// not JSON, or not an object at the top level
    Syntax,
    /// over [`MAX_ADVANCE_JSON_BYTES`] or [`MAX_ADVANCE_ENTRIES`]
    TooLarge,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AdvanceTable {
    sizes: FxHashMap<String, FxHashMap<u32, f64>>,
    entries: usize,
}

impl AdvanceTable {
    pub fn new() -> AdvanceTable {
        AdvanceTable::default()
    }

    /// The table of a `*-advances.json` text (what the viewer fetches next to a face).
    pub fn parse_json(text: &str) -> Result<AdvanceTable, AdvanceTableError> {
        if text.len() > MAX_ADVANCE_JSON_BYTES {
            return Err(AdvanceTableError::TooLarge);
        }
        let mut p = Parser { s: text.as_bytes(), at: 0, table: AdvanceTable::new() };
        p.ws();
        p.top()?;
        p.ws();
        if p.at != p.s.len() {
            return Err(AdvanceTableError::Syntax);
        }
        Ok(p.table)
    }

    /// Sets the advance of one character at one size (the host built the table in code). The
    /// last value for a pair wins.
    pub fn insert(&mut self, size_key: &str, code_point: u32, twips: f64) -> Result<(), AdvanceTableError> {
        if self.entries >= MAX_ADVANCE_ENTRIES {
            return Err(AdvanceTableError::TooLarge);
        }
        let sizes_before = self.sizes.len();
        let inner = self.sizes.entry(size_key.to_owned()).or_default();
        let added = inner.insert(code_point, twips).is_none();
        self.entries += usize::from(added) + usize::from(self.sizes.len() > sizes_before);
        Ok(())
    }

    /// `table[String(sizePt)]?.[String.fromCodePoint(codePoint)]`: `size_key` is the caller's
    /// `to_js_string(size_pt)`.
    pub fn get(&self, size_key: &str, code_point: u32) -> Option<f64> {
        self.sizes.get(size_key)?.get(&code_point).copied()
    }

    /// Entries held: sizes plus characters.
    pub fn len(&self) -> usize {
        self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries == 0
    }

    /// The heap of the table (an estimate from capacities, as `Face::heap_bytes`).
    pub fn heap_bytes(&self) -> usize {
        let inner: usize = self.sizes.iter().map(|(k, m)| k.capacity() + table_bytes::<(u32, f64)>(m.capacity())).sum();
        size_of::<AdvanceTable>() + table_bytes::<(String, FxHashMap<u32, f64>)>(self.sizes.capacity()) + inner
    }
}

struct Parser<'a> {
    s: &'a [u8],
    at: usize,
    table: AdvanceTable,
}

type Parsed<T> = Result<T, AdvanceTableError>;

impl Parser<'_> {
    fn ws(&mut self) {
        while matches!(self.s.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.at).copied()
    }

    fn eat(&mut self, c: u8) -> Parsed<()> {
        if self.peek() == Some(c) {
            self.at += 1;
            Ok(())
        } else {
            Err(AdvanceTableError::Syntax)
        }
    }

    fn literal(&mut self, word: &[u8]) -> Parsed<()> {
        if self.s.get(self.at..self.at + word.len()) == Some(word) {
            self.at += word.len();
            Ok(())
        } else {
            Err(AdvanceTableError::Syntax)
        }
    }

    /// The top-level object: size keys, each with an object of characters (any other value is skipped).
    fn top(&mut self) -> Parsed<()> {
        self.eat(b'{')?;
        self.ws();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(());
        }
        loop {
            self.ws();
            let size = self.string()?;
            self.ws();
            self.eat(b':')?;
            self.ws();
            let key = String::from_utf16_lossy(&size);
            let replaced = if self.peek() == Some(b'{') {
                let inner = self.characters()?;
                self.table.entries += inner.len() + 1;
                // JSON.parse: a repeated key replaces the whole value
                self.table.sizes.insert(key, inner)
            } else {
                self.skip(1)?;
                self.table.sizes.remove(&key)
            };
            if let Some(old) = replaced {
                self.table.entries -= old.len() + 1;
            }
            if self.table.entries > MAX_ADVANCE_ENTRIES {
                return Err(AdvanceTableError::TooLarge);
            }
            self.ws();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(());
                }
                _ => return Err(AdvanceTableError::Syntax),
            }
        }
    }

    fn characters(&mut self) -> Parsed<FxHashMap<u32, f64>> {
        let mut out: FxHashMap<u32, f64> = FxHashMap::default();
        self.eat(b'{')?;
        self.ws();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(out);
        }
        loop {
            self.ws();
            let key = self.string()?;
            self.ws();
            self.eat(b':')?;
            self.ws();
            let cp = single_code_point(&key);
            if matches!(self.peek(), Some(b'-' | b'0'..=b'9')) {
                let n = self.number()?;
                if let Some(cp) = cp {
                    out.insert(cp, n);
                }
            } else {
                self.skip(2)?;
                if let Some(cp) = cp {
                    out.remove(&cp);
                }
            }
            if out.len() > MAX_ADVANCE_ENTRIES {
                return Err(AdvanceTableError::TooLarge);
            }
            self.ws();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(out);
                }
                _ => return Err(AdvanceTableError::Syntax),
            }
        }
    }

    /// A JSON number, converted as JavaScript does.
    fn number(&mut self) -> Parsed<f64> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.peek() {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(AdvanceTableError::Syntax),
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(AdvanceTableError::Syntax);
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(AdvanceTableError::Syntax);
            }
            self.digits();
        }
        let text = self.s.get(start..self.at).and_then(|b| std::str::from_utf8(b).ok()).ok_or(AdvanceTableError::Syntax)?;
        Ok(to_number(text))
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
    }

    /// A JSON string as UTF-16 code units (a lone surrogate escape stays one unit).
    fn string(&mut self) -> Parsed<Vec<u16>> {
        self.eat(b'"')?;
        let mut out: Vec<u16> = Vec::new();
        loop {
            let b = self.peek().ok_or(AdvanceTableError::Syntax)?;
            match b {
                b'"' => {
                    self.at += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.at += 1;
                    let e = self.peek().ok_or(AdvanceTableError::Syntax)?;
                    self.at += 1;
                    let unit = match e {
                        b'"' => 0x22,
                        b'\\' => 0x5C,
                        b'/' => 0x2F,
                        b'b' => 0x08,
                        b'f' => 0x0C,
                        b'n' => 0x0A,
                        b'r' => 0x0D,
                        b't' => 0x09,
                        b'u' => {
                            let hex = self.s.get(self.at..self.at + 4).ok_or(AdvanceTableError::Syntax)?;
                            let mut v: u16 = 0;
                            for &h in hex {
                                let d = char::from(h).to_digit(16).ok_or(AdvanceTableError::Syntax)?;
                                v = (v << 4) | u16::try_from(d).map_err(|_| AdvanceTableError::Syntax)?;
                            }
                            self.at += 4;
                            v
                        }
                        _ => return Err(AdvanceTableError::Syntax),
                    };
                    out.push(unit);
                }
                0..=0x1F => return Err(AdvanceTableError::Syntax),
                _ => {
                    let rest = self.s.get(self.at..).ok_or(AdvanceTableError::Syntax)?;
                    let width = utf8_width(b);
                    let chunk = rest.get(..width).ok_or(AdvanceTableError::Syntax)?;
                    let text = std::str::from_utf8(chunk).map_err(|_| AdvanceTableError::Syntax)?;
                    out.extend(text.encode_utf16());
                    self.at += width;
                }
            }
            if out.len() > MAX_ADVANCE_JSON_BYTES {
                return Err(AdvanceTableError::TooLarge);
            }
        }
    }

    /// Skips one JSON value of any kind.
    fn skip(&mut self, depth: u32) -> Parsed<()> {
        if depth > MAX_DEPTH {
            return Err(AdvanceTableError::TooLarge);
        }
        match self.peek().ok_or(AdvanceTableError::Syntax)? {
            b'{' => {
                self.at += 1;
                self.ws();
                if self.peek() == Some(b'}') {
                    self.at += 1;
                    return Ok(());
                }
                loop {
                    self.ws();
                    self.string()?;
                    self.ws();
                    self.eat(b':')?;
                    self.ws();
                    self.skip(depth + 1)?;
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.at += 1,
                        Some(b'}') => {
                            self.at += 1;
                            return Ok(());
                        }
                        _ => return Err(AdvanceTableError::Syntax),
                    }
                }
            }
            b'[' => {
                self.at += 1;
                self.ws();
                if self.peek() == Some(b']') {
                    self.at += 1;
                    return Ok(());
                }
                loop {
                    self.ws();
                    self.skip(depth + 1)?;
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(());
                        }
                        _ => return Err(AdvanceTableError::Syntax),
                    }
                }
            }
            b'"' => self.string().map(|_| ()),
            b't' => self.literal(b"true"),
            b'f' => self.literal(b"false"),
            b'n' => self.literal(b"null"),
            _ => self.number().map(|_| ()),
        }
    }
}

fn utf8_width(first: u8) -> usize {
    match first {
        0xF0..=0xFF => 4,
        0xE0..=0xEF => 3,
        0xC0..=0xDF => 2,
        _ => 1,
    }
}

/// The code point a key stands for when `String.fromCodePoint(cp)` can equal it: one unit (a lone
/// surrogate too) or a valid surrogate pair.
fn single_code_point(units: &[u16]) -> Option<u32> {
    match *units {
        [u] => Some(u32::from(u)),
        [hi @ 0xD800..=0xDBFF, lo @ 0xDC00..=0xDFFF] => Some(0x10000 + ((u32::from(hi) - 0xD800) << 10) + (u32::from(lo) - 0xDC00)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_hosted_shape() {
        let t = AdvanceTable::parse_json(
            r#"{ "11": { "a": 91.5, " ": 46, "é": 90.25, "ab": 1 }, "10.5": { "a": -0.5e1 }, "x": 3, "y": [1, {"a": 2}] }"#,
        )
        .unwrap();
        assert_eq!(t.get("11", 'a' as u32), Some(91.5));
        assert_eq!(t.get("11", ' ' as u32), Some(46.0));
        assert_eq!(t.get("11", 0xE9), Some(90.25));
        assert_eq!(t.get("11", 'b' as u32), None, "a two-character key matches no code point");
        assert_eq!(t.get("10.5", 'a' as u32), Some(-5.0));
        assert_eq!(t.get("12", 'a' as u32), None);
        assert_eq!(t.get("x", 'a' as u32), None);
    }

    #[test]
    fn last_duplicate_wins_and_wrong_values_drop() {
        let t = AdvanceTable::parse_json(r#"{ "11": { "a": 1, "a": 2, "b": "x", "c": 3 }, "11": { "z": 9 } }"#).unwrap();
        assert_eq!(t.get("11", 'z' as u32), Some(9.0));
        assert_eq!(t.get("11", 'a' as u32), None, "a repeated size replaces the whole inner object");
        let t = AdvanceTable::parse_json(r#"{ "11": { "a": 1, "a": "no" } }"#).unwrap();
        assert_eq!(t.get("11", 'a' as u32), None);
    }

    #[test]
    fn astral_and_lone_surrogate_keys() {
        let t = AdvanceTable::parse_json(r#"{ "12": { "😀": 7, "\ud800": 8, "😀😀": 9 } }"#).unwrap();
        assert_eq!(t.get("12", 0x1F600), Some(7.0));
        assert_eq!(t.get("12", 0xD800), Some(8.0));
        assert_eq!(t.len(), 3);
    }

    #[test]
    fn malformed_json_is_an_error() {
        for bad in [
            "",
            "[]",
            "{",
            r#"{"a":}"#,
            r#"{"11":{"a":01}}"#,
            r#"{"11":{"a":1,}}"#,
            "{\"11\":{\"\u{1}\":1}}",
            r#"{"11":{"a":1}} x"#,
            r#"{"11":{"a":1.}}"#,
        ] {
            assert_eq!(AdvanceTable::parse_json(bad), Err(AdvanceTableError::Syntax), "{bad:?}");
        }
        let deep = format!("{{\"a\":{}1{}}}", "[".repeat(40), "]".repeat(40));
        assert_eq!(AdvanceTable::parse_json(&deep), Err(AdvanceTableError::TooLarge));
    }

    #[test]
    fn built_in_code() {
        let mut t = AdvanceTable::new();
        t.insert("11", 97, 1.0).unwrap();
        t.insert("11", 97, 2.0).unwrap();
        t.insert("12", 98, 3.0).unwrap();
        assert_eq!(t.get("11", 97), Some(2.0));
        assert_eq!(t.get("12", 98), Some(3.0));
        assert_eq!(t.len(), 4);
        assert!(t.heap_bytes() > 0);
    }
}
