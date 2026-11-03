//! `JSON.stringify` as a key and as equality (K9, LQ20): `bdrKey` (engine.ts:4089, borders
//! grouped by `JSON.stringify([pBdr, ind])`), `fxKey` (para.ts:249), the shaping key with
//! `JSON.stringify(r.ot)` (para.ts:476), `eq` (context.ts:217), the old-formatting comparisons
//! (revisions.ts:111, 122), and the PDF's picture/page-fill cache keys (uo-pdf).
//!
//! The text is built with the JS rules — keys in insertion order, `undefined` omitted in objects
//! and `null` in arrays, NaN and ±∞ as `null`, `-0` as `0`, strings escaped as V8 does (lone
//! surrogates as `\uXXXX`) — by `uo_jsnum::json` for the leaves and by the uo-docx `JsonWriter`
//! for model values (their key order comes from the A6 insertion-order facet, `KeyOrder`).
//! `derive(PartialEq)` is never used for these comparisons: it is the opposite for NaN and ignores
//! key order.

/// The `JSON.stringify` text of a value, used as a map key or compared for equality.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct JsonKey(pub Box<str>);

impl JsonKey {
    /// `JSON.stringify(a) === JSON.stringify(b)` (`eq`, context.ts:217).
    #[must_use]
    pub fn json_eq(a: &JsonKey, b: &JsonKey) -> bool {
        a.0 == b.0
    }
}

/// A builder of JSON text for values the layout composes itself (arrays and objects of model
/// values and numbers). Model values are written by the uo-docx `JsonWriter` (F2/F4 wire it in
/// through [`JsonBuilder::raw`]).
#[derive(Debug, Default)]
pub struct JsonBuilder {
    out: String,
    /// per open array/object: no element written yet
    first: Vec<bool>,
    /// a member key was just written: its value takes no separator
    pending_key: bool,
}

impl JsonBuilder {
    #[must_use]
    pub fn new() -> JsonBuilder {
        JsonBuilder::default()
    }

    fn sep(&mut self) {
        if self.pending_key {
            self.pending_key = false;
            return;
        }
        if let Some(f) = self.first.last_mut() {
            if *f {
                *f = false;
            } else {
                self.out.push(',');
            }
        }
    }

    pub fn begin_array(&mut self) -> &mut Self {
        self.sep();
        self.out.push('[');
        self.first.push(true);
        self
    }

    pub fn end_array(&mut self) -> &mut Self {
        self.first.pop();
        self.out.push(']');
        self
    }

    pub fn begin_object(&mut self) -> &mut Self {
        self.sep();
        self.out.push('{');
        self.first.push(true);
        self
    }

    pub fn end_object(&mut self) -> &mut Self {
        self.first.pop();
        self.out.push('}');
        self
    }

    /// An object member's key (its value follows; a member whose value is `undefined` is simply
    /// not written, as `JSON.stringify` omits it).
    pub fn key(&mut self, k: &str) -> &mut Self {
        self.sep();
        uo_jsnum::json::push_string(&mut self.out, k);
        self.out.push(':');
        self.pending_key = true;
        self
    }

    /// A number (NaN and ±∞ as `null`, `-0` as `0`).
    pub fn num(&mut self, x: f64) -> &mut Self {
        self.sep();
        uo_jsnum::json::push_number(&mut self.out, x);
        self
    }

    /// A string of UTF-16 code units (lone surrogates escaped).
    pub fn str16(&mut self, s: &[u16]) -> &mut Self {
        self.sep();
        uo_jsnum::json::push_string_utf16(&mut self.out, s);
        self
    }

    pub fn bool(&mut self, b: bool) -> &mut Self {
        self.sep();
        self.out.push_str(if b { "true" } else { "false" });
        self
    }

    /// `null` (an `undefined` array element).
    pub fn null(&mut self) -> &mut Self {
        self.sep();
        self.out.push_str("null");
        self
    }

    /// JSON text written by another writer (a model value through the uo-docx `JsonWriter`).
    pub fn raw(&mut self, json: &str) -> &mut Self {
        self.sep();
        self.out.push_str(json);
        self
    }

    #[must_use]
    pub fn finish(self) -> JsonKey {
        JsonKey(self.out.into_boxed_str())
    }
}
