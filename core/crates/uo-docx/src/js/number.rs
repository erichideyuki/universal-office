//! Numbers as the TS model holds them.

use super::{JsValue, JsonWriter};

/// A TS `number` field (A2, U4b): any `f64` (NaN, ±Infinity and `-0` included, as `parseInt` or
/// `Number` of a bad attribute stores them) or `null`, the state of a NaN/±Infinity after the
/// JSON round trip of an undo (milestone 2). `null` is `!== undefined` and 0 in arithmetic
/// ([`JsNumber::to_number`]). Absence is the `Option` around it.
///
/// Every number of the model uses it (the TS type says `number`; the reader decides what is
/// stored), which costs nothing: `Option<JsNumber>` is 16 bytes, like `Option<f64>`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JsNumber {
    Num(f64),
    Null,
}

impl Default for JsNumber {
    fn default() -> Self {
        JsNumber::Num(0.0)
    }
}

impl JsNumber {
    /// `ToNumber`: the value, `null` as 0.
    #[must_use]
    pub fn to_number(self) -> f64 {
        match self {
            JsNumber::Num(x) => x,
            JsNumber::Null => 0.0,
        }
    }

    /// The number when it is one (`None` for `null`).
    #[must_use]
    pub fn as_f64(self) -> Option<f64> {
        match self {
            JsNumber::Num(x) => Some(x),
            JsNumber::Null => None,
        }
    }

    #[must_use]
    pub fn is_null(self) -> bool {
        matches!(self, JsNumber::Null)
    }

    /// JS truthiness (`null`, 0, `-0` and NaN are falsy).
    #[must_use]
    pub fn truthy(self) -> bool {
        match self {
            JsNumber::Num(x) => uo_jsnum::conv::truthy(x),
            JsNumber::Null => false,
        }
    }
}

impl From<f64> for JsNumber {
    fn from(x: f64) -> Self {
        JsNumber::Num(x)
    }
}

impl From<i32> for JsNumber {
    fn from(x: i32) -> Self {
        JsNumber::Num(f64::from(x))
    }
}

impl From<u32> for JsNumber {
    fn from(x: u32) -> Self {
        JsNumber::Num(f64::from(x))
    }
}

impl JsValue for JsNumber {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        match *self {
            JsNumber::Num(x) => w.num(x),
            JsNumber::Null => w.null(),
        }
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        match *self {
            JsNumber::Num(x) => w.num(x),
            JsNumber::Null => w.raw("null"),
        }
        true
    }
    fn exact(&self, k: &mut super::ExactKey) -> bool {
        match *self {
            JsNumber::Num(x) => k.f64(x),
            JsNumber::Null => k.tag(b'z'),
        }
    }
}

/// A TS `T | null` field (`pgBorders?: PageBorders | null`, `line?: PicLine | null`): absence is
/// the `Option` around it.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Nullable<T> {
    #[default]
    Null,
    Val(T),
}

impl<T> Nullable<T> {
    /// The value when not `null`.
    pub fn as_val(&self) -> Option<&T> {
        match self {
            Nullable::Val(v) => Some(v),
            Nullable::Null => None,
        }
    }
}

impl<T: JsValue> JsValue for Nullable<T> {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        match self {
            Nullable::Val(v) => v.canon(w),
            Nullable::Null => w.null(),
        }
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        match self {
            Nullable::Val(v) => v.json(w),
            Nullable::Null => {
                w.raw("null");
                true
            }
        }
    }
    fn exact(&self, k: &mut super::ExactKey) -> bool {
        match self {
            Nullable::Val(v) => k.tag(b'v') && v.exact(k),
            Nullable::Null => k.tag(b'z'),
        }
    }
}

/// A number used as a key of a TS `Map` (note ids, `parseInt` results that can be NaN): equal by
/// SameValueZero (NaN equals NaN, `-0` is stored as `+0` as `Map.prototype.set` does).
#[derive(Debug, Clone, Copy)]
pub struct NumKey(f64);

impl NumKey {
    #[must_use]
    pub fn new(x: f64) -> NumKey {
        NumKey(if x == 0.0 {
            0.0
        } else if x.is_nan() {
            f64::NAN
        } else {
            x
        })
    }

    #[must_use]
    pub fn get(self) -> f64 {
        self.0
    }

    fn bits(self) -> u64 {
        if self.0.is_nan() { 0x7ff8_0000_0000_0000 } else { self.0.to_bits() }
    }
}

impl PartialEq for NumKey {
    fn eq(&self, other: &Self) -> bool {
        self.bits() == other.bits()
    }
}

impl Eq for NumKey {}

impl std::hash::Hash for NumKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.bits().hash(state);
    }
}

impl Default for NumKey {
    fn default() -> Self {
        NumKey(0.0)
    }
}

impl From<f64> for NumKey {
    fn from(x: f64) -> Self {
        NumKey::new(x)
    }
}

impl JsValue for NumKey {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.num(self.0);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.num(self.0);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn num_key_is_same_value_zero() {
        assert_eq!(NumKey::new(f64::NAN), NumKey::new(-f64::NAN));
        assert_eq!(NumKey::new(-0.0), NumKey::new(0.0));
        assert!(NumKey::new(-0.0).get().is_sign_positive());
        assert_ne!(NumKey::new(1.0), NumKey::new(2.0));
        assert_eq!(std::mem::size_of::<Option<JsNumber>>(), 16);
    }
}
