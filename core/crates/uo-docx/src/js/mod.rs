//! The JS value shapes the TS model can observe (architecture A2, A5), and the plumbing that
//! makes every model type answer the two JS questions the parity and the layout ask:
//!
//! - its **canonical digest** (A6, feature `canon`): [`crate::canon::CanonWriter`];
//! - its **`JSON.stringify`** with the JS rules (insertion order, `undefined` omitted, NaN and
//!   ±Infinity as `null`, `-0` as `0`): [`JsonWriter`], used by uo-docx-layout's `json_key`
//!   (LQ20) and by the D1b signatures.
//!
//! Model objects are declared with [`crate::js_object!`]: a struct of TS fields (`opt` = `?:`,
//! `req` = required) plus two bookkeeping fields every TS object needs here:
//! `cleared: FieldMask` (the key exists with the value `undefined`, U4) and `keys: KeyOrder` (the
//! insertion order of the keys when it is not the declaration order, K9/LQ20).

mod json;
mod number;
mod object;
mod record;
mod value;

pub use json::JsonWriter;
pub use number::{JsNumber, Nullable, NumKey};
pub use object::{FieldMask, JsObject, KeyOrder, Slot, object_json, object_json_with, sort_keys};
#[cfg(feature = "canon")]
pub use object::{object_canon, object_canon_with};
pub use record::{JsMap, JsRecord};
pub use value::JsonValue;

/// A TS string that cannot hold a lone surrogate (attribute values, names, ids read from XML:
/// the XML decoder never produces one). Text the user edits is [`crate::text::Utf16`].
pub type Str = Box<str>;

/// A value of the TS model: how it enters the canonical digest and `JSON.stringify`.
pub trait JsValue {
    /// Writes the value to the canonical stream (A6).
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>);

    /// Writes `JSON.stringify(value)`; returns `false` without writing anything when the value
    /// is `undefined` (the caller omits the key, or writes `null` in an array).
    fn json(&self, w: &mut JsonWriter<'_>) -> bool;
}

impl JsValue for bool {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.bool(*self);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.raw(if *self { "true" } else { "false" });
        true
    }
}

impl JsValue for f64 {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.num(*self);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.num(*self);
        true
    }
}

impl JsValue for str {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.str(self);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.str(self);
        true
    }
}

impl<T: JsValue + ?Sized> JsValue for Box<T> {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        (**self).canon(w);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        (**self).json(w)
    }
}

impl<T: JsValue + ?Sized> JsValue for std::sync::Arc<T> {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        (**self).canon(w);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        (**self).json(w)
    }
}

/// `T | undefined` as an array element or a map value: `None` is `undefined`.
impl<T: JsValue> JsValue for Option<T> {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        match self {
            Some(v) => v.canon(w),
            None => w.undefined(),
        }
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        match self {
            Some(v) => v.json(w),
            None => false,
        }
    }
}

impl<T: JsValue> JsValue for [T] {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.array(self.len());
        for v in self {
            v.canon(w);
        }
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.array(self.iter().map(|v| v as &dyn JsValue));
        true
    }
}

impl<T: JsValue> JsValue for Vec<T> {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        self.as_slice().canon(w);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        self.as_slice().json(w)
    }
}

impl<T: JsValue, const N: usize> JsValue for [T; N] {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        self.as_slice().canon(w);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        self.as_slice().json(w)
    }
}

/// A TS tuple `[a, b]` (an array of two).
impl<A: JsValue, B: JsValue> JsValue for (A, B) {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.array(2);
        self.0.canon(w);
        self.1.canon(w);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.array([&self.0 as &dyn JsValue, &self.1 as &dyn JsValue].into_iter());
        true
    }
}
