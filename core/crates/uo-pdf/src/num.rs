//! Numbers and strings of the content streams, exactly as the TS writes them (`document.ts`
//! `fmt`, the template strings, `ByteBuilder.str`, `utf16beHex`).

use std::fmt::{self, Write as _};

use uo_jsnum::format::Js;
use uo_jsnum::math;

/// The largest modulus written in a PDF number (no exponent notation from `String()`).
const MAX_REAL: f64 = 1e9;

/// The value `fmt` writes: NaN is 0, the modulus is clamped to 1e9 (so ±Infinity is ±1e9), the
/// result is rounded to 3 decimals with `Math.round` (ties toward +Infinity).
pub fn fmt_value(n: f64) -> f64 {
    if n.is_nan() {
        return 0.0;
    }
    let r = math::round(math::max(-MAX_REAL, math::min(MAX_REAL, n)) * 1000.0) / 1000.0;
    if r == 0.0 { 0.0 } else { r }
}

/// `fmt(n)` of the TS: up to 3 decimals, no trailing zeros, never an exponent, NaN, or Infinity.
pub fn fmt(n: f64) -> String {
    Js(fmt_value(n)).to_string()
}

/// A `Vec<u8>` that formats into itself: the bytes of what the TS builds with template strings.
pub(crate) struct Out<'a>(pub &'a mut Vec<u8>);

impl fmt::Write for Out<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        latin1(self.0, s);
        Ok(())
    }
}

/// Appends `s` as the TS `ByteBuilder.str` does: one byte per UTF-16 code unit, the low 8 bits.
pub(crate) fn latin1(out: &mut Vec<u8>, s: &str) {
    if s.is_ascii() {
        out.extend_from_slice(s.as_bytes());
    } else {
        out.extend(s.encode_utf16().map(|u| (u & 0xff) as u8));
    }
}

/// `utf16beHex`: each UTF-16 code unit as 4 lowercase hex digits, big-endian.
pub fn utf16be_hex_units(units: impl IntoIterator<Item = u16>) -> String {
    let mut out = String::new();
    for u in units {
        // writing into a String cannot fail
        let _ = write!(out, "{u:04x}");
    }
    out
}

/// `utf16beHex` of a string (its UTF-16 code units).
pub fn utf16be_hex(s: &str) -> String {
    utf16be_hex_units(s.encode_utf16())
}

/// `fmt(n)` as a `Display` value, to write into a content stream without a temporary string.
#[derive(Clone, Copy)]
pub(crate) struct F(pub f64);

impl fmt::Display for F {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Js(fmt_value(self.0)).fmt(f)
    }
}

/// `String(n)` of a JS number as a `Display` value (NaN, Infinity and exponents included).
#[derive(Clone, Copy)]
pub(crate) struct J(pub f64);

impl fmt::Display for J {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Js(self.0).fmt(f)
    }
}

/// The three components of a colour: `fmt(r) fmt(g) fmt(b)`.
#[derive(Clone, Copy)]
pub(crate) struct Rgb<'a>(pub &'a [f64; 3]);

impl fmt::Display for Rgb<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b] = *self.0;
        write!(f, "{} {} {}", F(r), F(g), F(b))
    }
}

/// `values.map(fmt).join(' ')`.
#[derive(Clone, Copy)]
pub(crate) struct FList<'a>(pub &'a [f64]);

impl fmt::Display for FList<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, v) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{}", F(*v))?;
        }
        Ok(())
    }
}

/// Formats into a `Vec<u8>` (writing into a vector cannot fail).
macro_rules! put {
    ($buf:expr, $($arg:tt)*) => {{
        use std::fmt::Write as _;
        let _ = write!($crate::num::Out(&mut $buf), $($arg)*);
    }};
}
pub(crate) use put;
