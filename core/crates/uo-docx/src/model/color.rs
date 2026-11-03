//! The Word model's DrawingML colours (G12): the TS keeps them as strings
//! (`"RRGGBB|lumMod=75000,…"`, `"@accent1|…"`, built by `srgbOf` in `drawing.ts`); here they are
//! the structured [`uo_drawingml::dml::WordSpec`], resolved by
//! [`uo_drawingml::dml::resolve_word_spec`]. The digest and `JSON.stringify` write the TS string
//! back ([`uo_drawingml::dml::WordSpec::to_ts_string`]), so the parity compares what the TS held.

use uo_drawingml::dml::{SpecHead, WordSpec};

use crate::js::{JsValue, JsonWriter};

/// A colour of the Word model in the TS spec form (`ShapeStyle.fill`, `line.color`,
/// `gradient.from`…): see the module comment.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawingColor(pub WordSpec);

impl Default for DrawingColor {
    fn default() -> Self {
        DrawingColor(WordSpec { head: SpecHead::Hex("".into()), mods: Vec::new(), alpha: None })
    }
}

impl DrawingColor {
    /// A plain colour string the TS stores as is (`'000000'`, a VML colour already converted):
    /// the head of a spec without modifiers. A string starting with `@` is a scheme reference.
    #[must_use]
    pub fn plain(s: &str) -> DrawingColor {
        let head = match s.strip_prefix('@') {
            Some(k) => SpecHead::Scheme(k.into()),
            None => SpecHead::Hex(s.into()),
        };
        DrawingColor(WordSpec { head, mods: Vec::new(), alpha: None })
    }
}

impl JsValue for DrawingColor {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.str(&self.0.to_ts_string());
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.str(&self.0.to_ts_string());
        true
    }
}
