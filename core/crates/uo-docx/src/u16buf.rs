//! A growing UTF-16 buffer: the JS strings the exporters and the compare keys build (D5a). Text of
//! the model is UTF-16 with lone surrogates kept (A1.4), and a surrogate pair split across two runs
//! joins when the runs are concatenated, so the output is built in code units and converted to
//! UTF-8 once, at the end, with every lone surrogate as U+FFFD (`TextEncoder`).

use uo_jsnum::format::to_js_string;

/// A JS string under construction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct U16Buf(Vec<u16>);

impl U16Buf {
    #[must_use]
    pub fn new() -> U16Buf {
        U16Buf::default()
    }

    /// Appends UTF-8 text.
    pub fn push_str(&mut self, s: &str) {
        self.0.extend(s.encode_utf16());
    }

    /// Appends code units (lone surrogates allowed).
    pub fn push_units(&mut self, u: &[u16]) {
        self.0.extend_from_slice(u);
    }

    pub fn push_unit(&mut self, u: u16) {
        self.0.push(u);
    }

    /// Appends `String(x)` of a number.
    pub fn push_num(&mut self, x: f64) {
        self.push_str(&to_js_string(x));
    }

    pub fn push_buf(&mut self, other: &U16Buf) {
        self.0.extend_from_slice(&other.0);
    }

    #[must_use]
    pub fn units(&self) -> &[u16] {
        &self.0
    }

    #[must_use]
    pub fn into_units(self) -> Vec<u16> {
        self.0
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

    /// UTF-8 with every lone surrogate as U+FFFD.
    #[must_use]
    pub fn into_string_lossy(self) -> String {
        String::from_utf16_lossy(&self.0)
    }
}

/// `String.prototype.trim` over code units (the JS white space set).
#[must_use]
pub fn trim_units(u: &[u16]) -> &[u16] {
    let is_space = uo_jsnum::text::is_js_space_unit;
    let start = u.iter().position(|&c| !is_space(c)).unwrap_or(u.len());
    let end = u.iter().rposition(|&c| !is_space(c)).map_or(start, |i| i + 1);
    u.get(start..end).unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_a_split_pair() {
        let mut b = U16Buf::new();
        b.push_units(&[0xd83d]);
        b.push_units(&[0xde00]);
        assert_eq!(b.into_string_lossy(), "\u{1f600}");
        let mut c = U16Buf::new();
        c.push_unit(0xd83d);
        assert_eq!(c.into_string_lossy(), "\u{fffd}");
    }

    #[test]
    fn trims_js_space() {
        let u: Vec<u16> = "\u{feff} a b\u{a0}\n".encode_utf16().collect();
        assert_eq!(trim_units(&u), "a b".encode_utf16().collect::<Vec<_>>().as_slice());
        assert_eq!(trim_units(&[0x20, 0x20]), &[] as &[u16]);
        assert_eq!(trim_units(&[]), &[] as &[u16]);
    }
}
