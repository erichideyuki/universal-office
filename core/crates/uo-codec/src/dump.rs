//! The canonical text dump the generated test-support code writes (`*_support.rs`), the form in
//! which Rust and TypeScript values are compared field by field: a `f64` is its bits, a string
//! is its code units in hex, a list is `[a,b]`, an absent optional is `-`.

use std::fmt::Write as _;

pub fn u(out: &mut String, v: impl std::fmt::Display) {
    let _ = write!(out, "{v}");
}

pub fn f64(out: &mut String, v: f64) {
    let _ = write!(out, "f:{:016x}", crate::canonical_f64_bits(v));
}

pub fn utf16(out: &mut String, units: &[u16]) {
    out.push('"');
    for u in units {
        let _ = write!(out, "{u:04x}");
    }
    out.push('"');
}

pub fn bytes(out: &mut String, bytes: &[u8]) {
    out.push_str("b\"");
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out.push('"');
}
