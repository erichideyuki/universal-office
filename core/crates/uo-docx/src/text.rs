//! Text of the model as UTF-16 code units (architecture A1.4, G6): positions are code units, a
//! lone surrogate (what Backspace after a non-BMP character leaves in the TS) is kept, and UTF-8
//! exists only at the XML boundaries (the reader decodes; the writer encodes a lone surrogate as
//! U+FFFD, as `TextEncoder`).

use crate::js::{JsValue, JsonWriter};

/// A JS string of the model (run text, field results, comment text): its UTF-16 code units.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
pub struct Utf16(Box<[u16]>);

impl Utf16 {
    #[must_use]
    pub fn new() -> Utf16 {
        Utf16::default()
    }

    /// From code units (lone surrogates allowed).
    #[must_use]
    pub fn from_units(units: Vec<u16>) -> Utf16 {
        Utf16(units.into_boxed_slice())
    }

    /// The code units.
    #[must_use]
    pub fn units(&self) -> &[u16] {
        &self.0
    }

    /// `s.length`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The text as UTF-8 with every lone surrogate as U+FFFD (`TextEncoder`).
    #[must_use]
    pub fn to_string_lossy(&self) -> String {
        String::from_utf16_lossy(&self.0)
    }

    /// Equal to a UTF-8 string (unit by unit).
    #[must_use]
    pub fn eq_str(&self, s: &str) -> bool {
        self.0.iter().copied().eq(s.encode_utf16())
    }

    /// Bytes used on the heap (M3 accounting).
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.0.len() * 2
    }
}

impl From<&str> for Utf16 {
    fn from(s: &str) -> Utf16 {
        Utf16(s.encode_utf16().collect())
    }
}

impl From<String> for Utf16 {
    fn from(s: String) -> Utf16 {
        Utf16::from(s.as_str())
    }
}

impl std::fmt::Debug for Utf16 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.to_string_lossy(), f)
    }
}

impl JsValue for Utf16 {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.str16(&self.0);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.str16(&self.0);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_lone_surrogates() {
        let t = Utf16::from_units(vec![0x61, 0xd83d]);
        assert_eq!(t.len(), 2);
        assert_eq!(t.to_string_lossy(), "a\u{fffd}");
        assert!(Utf16::from("a😀").eq_str("a😀"));
        assert_eq!(Utf16::from("a😀").len(), 3);
    }
}
