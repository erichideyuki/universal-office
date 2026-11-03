//! An arbitrary JSON value of the model (`CustomInline.data: unknown`, parsed from the
//! `wordnext:inline:` tag with `JSON.parse`).

use super::{JsRecord, JsValue, JsonWriter};
use crate::text::Utf16;

/// A value `JSON.parse` can return.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum JsonValue {
    #[default]
    Null,
    Bool(bool),
    Num(f64),
    Str(Utf16),
    Array(Vec<JsonValue>),
    Object(JsRecord<JsonValue>),
}

impl JsValue for JsonValue {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        match self {
            JsonValue::Null => w.null(),
            JsonValue::Bool(b) => w.bool(*b),
            JsonValue::Num(x) => w.num(*x),
            JsonValue::Str(s) => s.canon(w),
            JsonValue::Array(a) => a.canon(w),
            JsonValue::Object(o) => o.canon(w),
        }
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        match self {
            JsonValue::Null => w.raw("null"),
            JsonValue::Bool(b) => w.raw(if *b { "true" } else { "false" }),
            JsonValue::Num(x) => w.num(*x),
            JsonValue::Str(s) => w.str16(s.units()),
            JsonValue::Array(a) => return a.json(w),
            JsonValue::Object(o) => return o.json(w),
        }
        true
    }
}
