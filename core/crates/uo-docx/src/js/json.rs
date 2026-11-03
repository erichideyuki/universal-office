//! `JSON.stringify` of model values with the JS rules (uo-docx-layout LQ20 `json_key`).

use super::JsValue;
use crate::model::MediaStore;
use crate::model::sources::SourcePart;

/// Builds the `JSON.stringify` text of model values. Objects write their keys in insertion order
/// and omit `undefined`; arrays write `undefined` as `null`; numbers and strings follow
/// [`uo_jsnum::json`]; `Uint8Array`s (media bytes) are written as V8 does, an object of their
/// indices (`{"0":137,"1":80,…}`), which needs the document's [`MediaStore`].
pub struct JsonWriter<'a> {
    out: String,
    media: Option<&'a MediaStore>,
    parts: &'a [SourcePart],
    first: Vec<bool>,
}

impl<'a> JsonWriter<'a> {
    #[must_use]
    pub fn new(media: Option<&'a MediaStore>) -> JsonWriter<'a> {
        JsonWriter { out: String::new(), media, parts: &[], first: Vec::new() }
    }

    /// `JSON.stringify(v)` (`None` when `v` is `undefined`).
    pub fn stringify(v: &dyn JsValue, media: Option<&'a MediaStore>) -> Option<String> {
        let mut w = JsonWriter::new(media);
        v.json(&mut w).then_some(w.out)
    }

    /// The text written so far.
    #[must_use]
    pub fn finish(self) -> String {
        self.out
    }

    pub fn media(&self) -> Option<&'a MediaStore> {
        self.media
    }

    /// The source texts of the document (for `SourceStore` spans).
    #[must_use]
    pub fn with_parts(mut self, parts: &'a [SourcePart]) -> JsonWriter<'a> {
        self.parts = parts;
        self
    }

    pub fn parts(&self) -> &'a [SourcePart] {
        self.parts
    }

    /// Appends JSON text as is.
    pub fn raw(&mut self, s: &str) {
        self.out.push_str(s);
    }

    pub fn num(&mut self, x: f64) {
        uo_jsnum::json::push_number(&mut self.out, x);
    }

    pub fn str(&mut self, s: &str) {
        uo_jsnum::json::push_string(&mut self.out, s);
    }

    pub fn str16(&mut self, units: &[u16]) {
        uo_jsnum::json::push_string_utf16(&mut self.out, units);
    }

    /// `{`; members follow through [`JsonWriter::member`].
    pub fn begin_object(&mut self) {
        self.out.push('{');
        self.first.push(true);
    }

    /// `"key":value`, or nothing when the value is `undefined`.
    pub fn member(&mut self, key: &str, v: &dyn JsValue) {
        let mark = self.out.len();
        let first = self.first.last().copied().unwrap_or(true);
        if !first {
            self.out.push(',');
        }
        self.str(key);
        self.out.push(':');
        if v.json(self) {
            if let Some(f) = self.first.last_mut() {
                *f = false;
            }
        } else {
            self.out.truncate(mark);
        }
    }

    pub fn end_object(&mut self) {
        self.first.pop();
        self.out.push('}');
    }

    /// An array of the values (`undefined` elements as `null`).
    pub fn array<'v>(&mut self, items: impl Iterator<Item = &'v dyn JsValue>) {
        self.out.push('[');
        for (i, v) in items.enumerate() {
            if i > 0 {
                self.out.push(',');
            }
            if !v.json(self) {
                self.out.push_str("null");
            }
        }
        self.out.push(']');
    }

    /// A `Uint8Array` as V8 stringifies it.
    pub fn bytes(&mut self, b: &[u8]) {
        self.out.push('{');
        for (i, v) in b.iter().enumerate() {
            if i > 0 {
                self.out.push(',');
            }
            self.out.push('"');
            self.out.push_str(&i.to_string());
            self.out.push_str("\":");
            self.out.push_str(&v.to_string());
        }
        self.out.push('}');
    }
}
