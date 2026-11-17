//! Colour values as the TS keeps them: **strings**. `colorFromDml` (theme.ts:85) returns
//! `'#' + val` as the file wrote it when no modifier touched the colour (lower case, short or
//! invalid values included) and the uppercase `hex()` of the result otherwise; the painter assigns
//! the string to the canvas unparsed. Equality and the serialized form are therefore the string's.

/// A CSS colour string. `#RRGGBB` (the common case, 7 ASCII bytes) is stored inline; anything
/// else (`#abc`, `#zzzzzz`, `rgba(…)` built by the emitter) is boxed.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum CssColor {
    Hex7([u8; 7]),
    Other(Box<str>),
}

impl CssColor {
    /// Wraps a colour string exactly as given.
    pub fn new(s: &str) -> CssColor {
        match <[u8; 7]>::try_from(s.as_bytes()) {
            Ok(b) if b[0] == b'#' => CssColor::Hex7(b),
            _ => CssColor::Other(s.into()),
        }
    }

    /// The string the TS would hold.
    pub fn as_str(&self) -> &str {
        match self {
            // the inline form is only built from a `&str`, so it is valid UTF-8
            CssColor::Hex7(b) => core::str::from_utf8(b).unwrap_or("#000000"),
            CssColor::Other(s) => s,
        }
    }
}
