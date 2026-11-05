//! The exact identity of a model value ([`super::JsValue::exact`]): a byte string two values share
//! only when nothing — the TS, the digest, `JSON.stringify`, the writer — can tell them apart. The
//! reader's props interning keys on it (record `uo-docx.md`, "Model size — P1 done").
//!
//! "Exact" is stricter than `PartialEq`: numbers by their `f64` bits (`-0` and `0` differ, a NaN
//! equals only a NaN with the same bits), every key of a declared object with its state (absent,
//! `undefined` — the cleared mask —, a value), the whole cleared mask and the recorded key order. The
//! encoding is length-prefixed and tagged, so it is injective for a given type. A type that does not
//! declare one (the default of [`super::JsValue::exact`]) makes the value not internable.

/// The byte string of [`super::JsValue::exact`], bounded by a maximum length.
#[derive(Debug, Clone)]
pub struct ExactKey {
    bytes: Vec<u8>,
    max: usize,
}

impl ExactKey {
    /// An empty key that refuses values longer than `max` bytes.
    #[must_use]
    pub fn new(max: usize) -> ExactKey {
        ExactKey { bytes: Vec::new(), max }
    }

    /// Starts a new key, keeping the buffer.
    pub fn clear(&mut self) {
        self.bytes.clear();
    }

    /// The key written so far.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Whether the key is still within its maximum length (a value that writes past it is not
    /// internable: `exact` returns `false`).
    #[must_use]
    pub fn fits(&self) -> bool {
        self.bytes.len() <= self.max
    }

    /// A tag byte.
    pub fn tag(&mut self, t: u8) -> bool {
        self.bytes.push(t);
        self.fits()
    }

    pub fn u32(&mut self, x: u32) -> bool {
        self.bytes.extend_from_slice(&x.to_le_bytes());
        self.fits()
    }

    pub fn u64(&mut self, x: u64) -> bool {
        self.bytes.extend_from_slice(&x.to_le_bytes());
        self.fits()
    }

    /// A length (as `u64`, so the key is the same on 32- and 64-bit targets).
    pub fn count(&mut self, n: usize) -> bool {
        self.u64(n as u64)
    }

    /// A number by its bits.
    pub fn f64(&mut self, x: f64) -> bool {
        self.tag(b'f') && self.u64(x.to_bits())
    }

    /// A string: its length and its UTF-8 bytes.
    pub fn str(&mut self, s: &str) -> bool {
        if s.len() > self.max.saturating_sub(self.bytes.len()) {
            return false;
        }
        self.tag(b's') && self.count(s.len()) && {
            self.bytes.extend_from_slice(s.as_bytes());
            self.fits()
        }
    }

    /// UTF-16 text: its length and its code units (lone surrogates included).
    pub fn str16(&mut self, units: &[u16]) -> bool {
        if units.len().saturating_mul(2) > self.max.saturating_sub(self.bytes.len()) {
            return false;
        }
        if !(self.tag(b'u') && self.count(units.len())) {
            return false;
        }
        for u in units {
            self.bytes.extend_from_slice(&u.to_le_bytes());
        }
        self.fits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_by_bits_and_bounded_length() {
        let key = |f: &dyn Fn(&mut ExactKey) -> bool| {
            let mut k = ExactKey::new(32);
            f(&mut k).then(|| k.bytes().to_vec())
        };
        assert_ne!(key(&|k| k.f64(0.0)), key(&|k| k.f64(-0.0)));
        assert_eq!(key(&|k| k.f64(f64::NAN)), key(&|k| k.f64(f64::NAN)));
        assert_ne!(key(&|k| k.f64(f64::NAN)), key(&|k| k.f64(f64::from_bits(f64::NAN.to_bits() | 1))));
        assert_ne!(key(&|k| k.str("ab") && k.str("c")), key(&|k| k.str("a") && k.str("bc")));
        assert!(key(&|k| k.str(&"x".repeat(31))).is_none());
        assert!(key(&|k| k.str16(&[0xd800; 20])).is_none());
        assert!(key(&|k| k.str16(&[0xd800, 0x61])).is_some());
    }
}
