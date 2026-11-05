//! The JS value rules the writer's string building relies on: `String(n)`, `!==`, `??`, relational
//! comparisons and truthiness of `number | null | undefined` fields (`Option<JsNumber>`), and
//! `JSON.stringify` equality (`sameJson`, lossless.ts:292) in the signature mode (G23).

use crate::js::{JsNumber, JsValue, JsonWriter, SigEncoding};
use crate::write::sig::SigMode;

/// `String(n)` of a model number (`null` prints `null`).
#[must_use]
pub fn num_str(x: JsNumber) -> String {
    match x {
        JsNumber::Num(v) => uo_jsnum::format::to_js_string(v),
        JsNumber::Null => "null".to_owned(),
    }
}

/// `${x}` of an optional number (`undefined` prints `undefined`).
#[must_use]
pub fn opt_num_str(x: Option<JsNumber>) -> String {
    x.map_or_else(|| "undefined".to_owned(), num_str)
}

/// `a !== b` for `number | null | undefined`.
#[must_use]
pub fn strict_ne(a: Option<JsNumber>, b: Option<JsNumber>) -> bool {
    match (a, b) {
        (None, None) => false,
        (Some(JsNumber::Null), Some(JsNumber::Null)) => false,
        (Some(JsNumber::Num(x)), Some(JsNumber::Num(y))) => x != y,
        _ => true,
    }
}

/// `x ?? d` for a number field (`null` and `undefined` give `d`).
#[must_use]
pub fn or_num(x: Option<JsNumber>, d: f64) -> f64 {
    match x {
        Some(JsNumber::Num(v)) => v,
        _ => d,
    }
}

/// `ToNumber(x)` for a relational comparison (`undefined` is NaN, `null` is 0).
#[must_use]
pub fn rel_num(x: Option<JsNumber>) -> f64 {
    match x {
        None => f64::NAN,
        Some(v) => v.to_number(),
    }
}

/// JS truthiness of an optional number.
#[must_use]
pub fn truthy(x: Option<JsNumber>) -> bool {
    x.is_some_and(JsNumber::truthy)
}

/// JS truthiness of an optional string.
#[must_use]
pub fn truthy_str(x: Option<&str>) -> bool {
    x.is_some_and(|s| !s.is_empty())
}

/// `JSON.stringify(a ?? null) === JSON.stringify(b ?? null)` (`sameJson`), with the keys of
/// declared objects sorted in [`SigMode::Canonical`] (G23) and in insertion order in
/// [`SigMode::Ts`].
#[must_use]
pub fn same_json(a: Option<&dyn JsValue>, b: Option<&dyn JsValue>, mode: SigMode) -> bool {
    json(a, mode) == json(b, mode)
}

fn json(v: Option<&dyn JsValue>, mode: SigMode) -> String {
    let Some(v) = v else { return "null".to_owned() };
    let enc = SigEncoding { sorted_keys: mode == SigMode::Canonical };
    // a signature writer only changes how bytes are written; model props hold none
    let mut w = JsonWriter::for_signature(None, enc);
    if v.json(&mut w) { w.finish() } else { "null".to_owned() }
}

/// `Option<&T>` as `Option<&dyn JsValue>`.
pub fn dynv<T: JsValue>(x: Option<&T>) -> Option<&dyn JsValue> {
    x.map(|v| v as &dyn JsValue)
}

/// `esc` of lossless.ts and table-xml.ts: `& < > "`.
#[must_use]
pub fn esc_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    push_esc_attr(&mut out, s);
    out
}

/// Appends [`esc_attr`] of `s`.
pub fn push_esc_attr(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

/// `esc` of serialize.ts (text): `& < >`.
#[must_use]
pub fn esc_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}
